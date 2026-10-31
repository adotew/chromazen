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
mod tests;
