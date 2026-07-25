use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct PressureStateHandle(Arc<Mutex<PressureState>>);

#[derive(Debug)]
struct PressureState {
    pressure: f32,
    pen_active: bool,
}

impl Default for PressureState {
    fn default() -> Self {
        Self {
            pressure: 1.0,
            pen_active: false,
        }
    }
}

impl PressureStateHandle {
    pub fn brush_pressure(&self) -> f32 {
        let state = self.0.lock().expect("pressure state poisoned");
        if state.pen_active {
            state.pressure
        } else {
            1.0
        }
    }

    pub(crate) fn note_pen_pressure(&self, pressure: f32, active: bool) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let pressure = pressure.clamp(0.0, 1.0);
        let changed =
            state.pen_active != active || (state.pressure - pressure).abs() > f32::EPSILON;
        state.pen_active = active;
        state.pressure = pressure;
        changed
    }

    pub(crate) fn clear_pen(&self) -> bool {
        let mut state = self.0.lock().expect("pressure state poisoned");
        let changed = state.pen_active || (state.pressure - 1.0).abs() > f32::EPSILON;
        state.pen_active = false;
        state.pressure = 1.0;
        changed
    }
}
