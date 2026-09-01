use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Debug, Default)]
pub struct PressureStateHandle(Arc<Mutex<PressureState>>);

#[derive(Debug)]
struct PressureState {
    pressure: f32,
    pen_active: bool,
    pen_in_proximity: bool,
    sample_time: Option<Duration>,
}

impl Default for PressureState {
    fn default() -> Self {
        Self {
            pressure: 1.0,
            pen_active: false,
            pen_in_proximity: false,
            sample_time: None,
        }
    }
}

impl PressureState {
    fn brush_pressure(&self) -> f32 {
        if self.pen_active {
            self.pressure
        } else if self.pen_in_proximity {
            0.0
        } else {
            1.0
        }
    }
}

impl PressureStateHandle {
    pub fn brush_pressure(&self) -> f32 {
        self.brush_sample().0
    }

    pub(crate) fn brush_sample(&self) -> (f32, Option<Duration>) {
        let state = self.0.lock().expect("pressure state poisoned");
        (state.brush_pressure(), state.sample_time)
    }

    pub(crate) fn pen_input_active(&self) -> bool {
        let state = self.0.lock().expect("pressure state poisoned");
        state.pen_active || state.pen_in_proximity
    }

    pub(crate) fn stroke_pressure(&self, uses_pen_pressure: bool) -> f32 {
        if uses_pen_pressure {
            let state = self.0.lock().expect("pressure state poisoned");
            if state.pen_active {
                state.pressure
            } else {
                0.0
            }
        } else {
            1.0
        }
    }

    pub(crate) fn note_pen_pressure_at(
        &self,
        pressure: f32,
        active: bool,
        is_pen_device: bool,
        sample_time: Option<Duration>,
    ) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        state.pressure = pressure.clamp(0.0, 1.0);
        state.pen_active = active;
        state.pen_in_proximity |= is_pen_device;
        state.sample_time = sample_time;
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }

    pub(crate) fn end_pen_contact(&self, is_pen_device: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        state.pressure = 0.0;
        state.pen_active = false;
        state.pen_in_proximity |= is_pen_device;
        state.sample_time = None;
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }

    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn set_pen_proximity(&self, in_proximity: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        state.pressure = 0.0;
        state.pen_active = false;
        state.pen_in_proximity = in_proximity;
        state.sample_time = None;
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }

    pub(crate) fn reset_pen_state(&self) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let before = state.brush_pressure();
        *state = PressureState::default();
        (state.brush_pressure() - before).abs() > f32::EPSILON
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pen_hover_uses_minimum_pressure_and_mouse_uses_full_pressure() {
        let pressure = PressureStateHandle::default();
        assert_eq!(pressure.brush_pressure(), 1.0);

        pressure.set_pen_proximity(true);
        assert_eq!(pressure.brush_pressure(), 0.0);

        let sample_time = Duration::from_millis(12);
        pressure.note_pen_pressure_at(0.4, true, true, Some(sample_time));
        assert_eq!(pressure.brush_sample(), (0.4, Some(sample_time)));

        pressure.end_pen_contact(true);
        assert_eq!(pressure.brush_pressure(), 0.0);

        pressure.reset_pen_state();
        assert_eq!(pressure.brush_pressure(), 1.0);
    }

    #[test]
    fn pen_stroke_does_not_fall_back_to_full_mouse_pressure() {
        let pressure = PressureStateHandle::default();
        pressure.note_pen_pressure_at(0.4, true, true, None);
        assert!(pressure.pen_input_active());
        assert_eq!(pressure.stroke_pressure(true), 0.4);

        pressure.reset_pen_state();
        assert!(!pressure.pen_input_active());
        assert_eq!(pressure.stroke_pressure(true), 0.0);
        assert_eq!(pressure.stroke_pressure(false), 1.0);
    }
}
