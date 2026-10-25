use serde::{Deserialize, Serialize};

/// Maps raw pen pressure to brush pressure with a CSS-style cubic Bézier from (0, 0) to
/// (1, 1) whose `controls` are relative to the `zero_until..full_from` span. Keeping every
/// control coordinate in 0..=1 makes the curve non-decreasing.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PressureCurve {
    pub(crate) zero_until: f32,
    pub(crate) full_from: f32,
    pub(crate) controls: [[f32; 2]; 2],
}

impl Default for PressureCurve {
    fn default() -> Self {
        Self {
            zero_until: 0.0,
            full_from: 1.0,
            controls: [[1.0 / 3.0, 1.0 / 3.0], [2.0 / 3.0, 2.0 / 3.0]],
        }
    }
}

impl PressureCurve {
    pub(crate) const EDITOR_MIN_SPAN: f32 = 0.05;

    pub(crate) fn apply(&self, pressure: f32) -> f32 {
        if pressure <= self.zero_until {
            return 0.0;
        }
        if pressure >= self.full_from {
            return 1.0;
        }
        let x = (pressure - self.zero_until) / (self.full_from - self.zero_until);
        let [[x1, y1], [x2, y2]] = self.controls;
        bezier(non_decreasing_bezier_inverse(x, x1, x2), y1, y2).clamp(0.0, 1.0)
    }

    pub(crate) fn pen_and_brush_pressure_at(&self, t: f32) -> [f32; 2] {
        let [[x1, y1], [x2, y2]] = self.controls;
        [
            self.zero_until + (self.full_from - self.zero_until) * bezier(t, x1, x2),
            bezier(t, y1, y2),
        ]
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        let unit = |value: f32| (0.0..=1.0).contains(&value);
        if !unit(self.zero_until) || !unit(self.full_from) || self.zero_until >= self.full_from {
            return Err(
                "pressure_curve zero_until must be below full_from, both in 0..=1".to_owned(),
            );
        }
        if !self.controls.iter().flatten().all(|&value| unit(value)) {
            return Err("pressure_curve controls must lie in 0..=1".to_owned());
        }
        Ok(())
    }
}

fn non_decreasing_bezier_inverse(value: f32, p1: f32, p2: f32) -> f32 {
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..20 {
        let t = (low + high) / 2.0;
        if bezier(t, p1, p2) < value {
            low = t;
        } else {
            high = t;
        }
    }
    (low + high) / 2.0
}

fn bezier(t: f32, p1: f32, p2: f32) -> f32 {
    let u = 1.0 - t;
    3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
}

#[cfg(test)]
mod tests {
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
}
