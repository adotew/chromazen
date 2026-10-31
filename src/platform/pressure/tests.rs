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
