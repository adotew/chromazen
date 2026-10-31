use super::*;

#[test]
fn tablet_cursor_follows_visibility_changes() {
    let mut cursor = ToolCursorVisibility::default();
    assert_eq!(cursor.update_serial(false), None);
    cursor.enter(Some(10));
    assert_eq!(cursor.update_serial(false), Some(10));
    cursor.applied = Some(false);
    assert_eq!(cursor.update_serial(false), None);
    assert_eq!(cursor.update_serial(true), Some(10));
    cursor.applied = Some(true);
    assert_eq!(cursor.update_serial(true), None);
    assert_eq!(cursor.update_serial(false), Some(10));
}

#[test]
fn tablet_cursor_is_reapplied_with_new_proximity_serial() {
    let mut cursor = ToolCursorVisibility::default();
    cursor.enter(Some(10));
    cursor.applied = Some(false);
    cursor.enter(None);
    assert_eq!(cursor.update_serial(true), None);
    cursor.enter(Some(20));
    assert_eq!(cursor.update_serial(false), Some(20));
}

#[test]
fn frame_combines_motion_with_latest_pressure() {
    let mut frame = TabletFrame {
        proximity: true,
        tip_down: true,
        position: [24.0, 48.0],
        pressure: 0.25,
        pressure_supported: true,
        updated: true,
        ..TabletFrame::default()
    };

    assert_eq!(
        frame.take_pen_event(),
        Some(PenEvent::Motion {
            position: [24.0, 48.0],
            pressure: 0.25,
            contact: true,
            time: std::time::Duration::ZERO,
        })
    );
    assert!(!frame.updated);
}

#[test]
fn pen_without_pressure_uses_mouse_fallback_during_contact() {
    let mut frame = TabletFrame {
        proximity: true,
        tip_down: true,
        position: [10.0, 20.0],
        action: FrameAction::Down,
        ..TabletFrame::default()
    };

    assert_eq!(
        frame.take_pen_event(),
        Some(PenEvent::Down {
            position: [10.0, 20.0],
            pressure: 1.0,
            time: std::time::Duration::ZERO,
        })
    );
}

#[test]
fn up_and_leave_end_contact() {
    let mut frame = TabletFrame {
        proximity: true,
        tip_down: false,
        pressure: 0.8,
        action: FrameAction::Up,
        ..TabletFrame::default()
    };
    assert_eq!(frame.take_pen_event(), Some(PenEvent::Up));
    assert_eq!(frame.pressure, 0.0);

    frame.action = FrameAction::Leave;
    assert_eq!(frame.take_pen_event(), Some(PenEvent::Leave));
}
