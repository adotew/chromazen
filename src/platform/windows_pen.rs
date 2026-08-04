#![cfg_attr(not(test), allow(dead_code))]

use super::PenEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowsPenAction {
    Down,
    Motion,
    Up,
    Leave,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WindowsPenSample {
    pub action: WindowsPenAction,
    /// Surface-local logical coordinates.
    pub position: [f32; 2],
    pub pressure: Option<f32>,
    pub contact: bool,
}

pub(super) fn events_for_sample(sample: WindowsPenSample) -> Vec<PenEvent> {
    let pressure = effective_pressure(sample.pressure, sample.contact);
    match sample.action {
        WindowsPenAction::Down => vec![PenEvent::Down {
            position: sample.position,
            pressure,
        }],
        WindowsPenAction::Motion => vec![PenEvent::Motion {
            position: sample.position,
            pressure,
            contact: sample.contact,
        }],
        WindowsPenAction::Up => vec![PenEvent::Up],
        WindowsPenAction::Leave => vec![PenEvent::Leave],
        WindowsPenAction::Cancel => vec![PenEvent::Up, PenEvent::Leave],
    }
}

pub(super) fn normalize_pressure(raw: u32, supported: bool) -> Option<f32> {
    supported.then(|| (raw as f32 / 1024.0).clamp(0.0, 1.0))
}

fn effective_pressure(pressure: Option<f32>, contact: bool) -> f32 {
    if contact {
        pressure.unwrap_or(1.0).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_pressure_range() {
        assert_eq!(normalize_pressure(0, true), Some(0.0));
        assert_eq!(normalize_pressure(512, true), Some(0.5));
        assert_eq!(normalize_pressure(1024, true), Some(1.0));
        assert_eq!(normalize_pressure(2048, true), Some(1.0));
        assert_eq!(normalize_pressure(512, false), None);
    }

    #[test]
    fn pressureless_pen_uses_full_pressure_only_during_contact() {
        let down = WindowsPenSample {
            action: WindowsPenAction::Down,
            position: [12.0, 24.0],
            pressure: None,
            contact: true,
        };
        assert_eq!(
            events_for_sample(down),
            vec![PenEvent::Down {
                position: [12.0, 24.0],
                pressure: 1.0,
            }]
        );

        let hover = WindowsPenSample {
            action: WindowsPenAction::Motion,
            contact: false,
            ..down
        };
        assert_eq!(
            events_for_sample(hover),
            vec![PenEvent::Motion {
                position: [12.0, 24.0],
                pressure: 0.0,
                contact: false,
            }]
        );
    }

    #[test]
    fn release_leave_and_cancellation_end_input() {
        let sample = WindowsPenSample {
            action: WindowsPenAction::Up,
            position: [0.0; 2],
            pressure: Some(0.7),
            contact: false,
        };
        assert_eq!(events_for_sample(sample), vec![PenEvent::Up]);
        assert_eq!(
            events_for_sample(WindowsPenSample {
                action: WindowsPenAction::Leave,
                ..sample
            }),
            vec![PenEvent::Leave]
        );
        assert_eq!(
            events_for_sample(WindowsPenSample {
                action: WindowsPenAction::Cancel,
                ..sample
            }),
            vec![PenEvent::Up, PenEvent::Leave]
        );
    }
}
