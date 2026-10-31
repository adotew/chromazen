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
        time: Duration::from_millis(12),
    };
    assert_eq!(
        events_for_sample(down),
        vec![PenEvent::Down {
            position: [12.0, 24.0],
            pressure: 1.0,
            time: Duration::from_millis(12),
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
            time: Duration::from_millis(12),
        }]
    );
}

#[test]
fn release_and_cancellation_end_input() {
    let sample = WindowsPenSample {
        action: WindowsPenAction::Up,
        position: [0.0; 2],
        pressure: Some(0.7),
        contact: false,
        time: Duration::from_millis(18),
    };
    assert_eq!(events_for_sample(sample), vec![PenEvent::Up]);
    assert_eq!(
        events_for_sample(WindowsPenSample {
            action: WindowsPenAction::Cancel,
            ..sample
        }),
        vec![PenEvent::Up, PenEvent::Leave]
    );
}

#[test]
fn physical_coordinates_follow_windows_dpi() {
    assert_eq!(physical_to_logical([300, 150], 96), [300.0, 150.0]);
    assert_eq!(physical_to_logical([300, 150], 144), [200.0, 100.0]);
    assert_eq!(physical_to_logical([300, 150], 0), [300.0, 150.0]);
}
