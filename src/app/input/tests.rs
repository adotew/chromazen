use super::*;

#[test]
fn rotation_snaps_to_quarter_turns_with_hysteresis() {
    let near_ninety = 87.0_f32.to_radians();
    let (rotation, snap) = snapped_rotation(near_ninety, None);
    assert_eq!(rotation, std::f32::consts::FRAC_PI_2);
    assert_eq!(snap, Some(std::f32::consts::FRAC_PI_2));

    let still_close = 96.0_f32.to_radians();
    assert_eq!(
        snapped_rotation(still_close, snap),
        (
            std::f32::consts::FRAC_PI_2,
            Some(std::f32::consts::FRAC_PI_2)
        )
    );

    let released = 99.0_f32.to_radians();
    assert_eq!(snapped_rotation(released, snap), (released, None));
}

#[test]
fn angular_drag_crosses_the_signed_angle_boundary() {
    let origin = 179.0_f32.to_radians();
    let current = -179.0_f32.to_radians();
    assert!((angle_delta(current, origin) - 2.0_f32.to_radians()).abs() < 0.0001);
}

#[test]
fn maps_tab_shortcuts() {
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::Tab,
            ElementState::Pressed,
            false,
            ModifiersState::empty(),
        ),
        Some(KeyboardShortcut::ToggleSidebar)
    );
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::Tab,
            ElementState::Pressed,
            false,
            ModifiersState::SHIFT,
        ),
        Some(KeyboardShortcut::CycleTool)
    );
}

#[test]
fn maps_canvas_flip_shortcuts() {
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::KeyF,
            ElementState::Pressed,
            false,
            ModifiersState::empty(),
        ),
        Some(KeyboardShortcut::FlipCanvasHorizontal)
    );
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::KeyF,
            ElementState::Pressed,
            false,
            ModifiersState::SHIFT,
        ),
        Some(KeyboardShortcut::FlipCanvasVertical)
    );
}

#[test]
fn ignores_unhandled_tab_variants() {
    for modifiers in [
        ModifiersState::CONTROL,
        ModifiersState::ALT,
        ModifiersState::SUPER,
        ModifiersState::SHIFT | ModifiersState::CONTROL,
    ] {
        assert_eq!(
            keyboard_shortcut_for_key(KeyCode::Tab, ElementState::Pressed, false, modifiers,),
            None
        );
    }
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::Tab,
            ElementState::Released,
            false,
            ModifiersState::empty(),
        ),
        None
    );
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::Tab,
            ElementState::Pressed,
            true,
            ModifiersState::empty(),
        ),
        None
    );
    assert_eq!(
        keyboard_shortcut_for_key(
            KeyCode::KeyR,
            ElementState::Pressed,
            false,
            ModifiersState::empty(),
        ),
        None
    );
}

#[test]
fn cycles_tools_and_wraps() {
    let mut input = PaintInputController::default();
    assert!(input.cycle_tool());
    assert_eq!(input.tool(), PaintTool::Eraser.into());
    assert!(input.cycle_tool());
    assert_eq!(input.tool(), PaintTool::Smudge.into());
    assert!(input.cycle_tool());
    assert_eq!(input.tool(), PaintTool::Brush.into());
}

#[test]
fn cycling_is_blocked_during_a_stroke() {
    let mut input = PaintInputController {
        is_drawing: true,
        ..Default::default()
    };
    assert!(!input.cycle_tool());
    assert_eq!(input.tool(), PaintTool::Brush.into());
}

#[test]
fn only_unmodified_option_enables_eyedropper() {
    assert!(eyedropper_modifier_is_active(ModifiersState::ALT));
    for modifiers in [
        ModifiersState::empty(),
        ModifiersState::SHIFT,
        ModifiersState::CONTROL,
        ModifiersState::SUPER,
        ModifiersState::ALT | ModifiersState::SHIFT,
        ModifiersState::ALT | ModifiersState::CONTROL,
        ModifiersState::ALT | ModifiersState::SUPER,
    ] {
        assert!(!eyedropper_modifier_is_active(modifiers));
    }
}

#[test]
fn drag_sampling_is_throttled() {
    let now = Instant::now();
    assert!(color_sample_is_due(None, now));
    assert!(!color_sample_is_due(Some(now), now));
    assert!(!color_sample_is_due(
        Some(now),
        now + EYEDROPPER_DRAG_SAMPLE_INTERVAL - Duration::from_millis(1),
    ));
    assert!(color_sample_is_due(
        Some(now),
        now + EYEDROPPER_DRAG_SAMPLE_INTERVAL,
    ));
}

#[test]
fn only_unmodified_shift_enables_resize() {
    assert!(resize_modifier_is_active(ModifiersState::SHIFT));
    for modifiers in [
        ModifiersState::empty(),
        ModifiersState::CONTROL,
        ModifiersState::ALT,
        ModifiersState::SUPER,
        ModifiersState::SHIFT | ModifiersState::CONTROL,
        ModifiersState::SHIFT | ModifiersState::ALT,
        ModifiersState::SHIFT | ModifiersState::SUPER,
    ] {
        assert!(!resize_modifier_is_active(modifiers));
    }
}

#[test]
fn maps_tool_shortcuts() {
    assert_eq!(
        editor_tool_for_key(KeyCode::KeyD, ModifiersState::empty()),
        Some(PaintTool::Brush.into())
    );
    assert_eq!(
        editor_tool_for_key(KeyCode::KeyB, ModifiersState::empty()),
        Some(PaintTool::Brush.into())
    );
    assert_eq!(
        editor_tool_for_key(KeyCode::KeyE, ModifiersState::SHIFT),
        Some(PaintTool::Eraser.into())
    );
    assert_eq!(
        editor_tool_for_key(KeyCode::KeyS, ModifiersState::empty()),
        Some(PaintTool::Smudge.into())
    );
    assert_eq!(
        editor_tool_for_key(KeyCode::KeyT, ModifiersState::empty()),
        Some(EditorTool::Transform)
    );
    assert_eq!(
        editor_tool_for_key(KeyCode::KeyL, ModifiersState::empty()),
        Some(EditorTool::Select)
    );
    for modifiers in [
        ModifiersState::CONTROL,
        ModifiersState::ALT,
        ModifiersState::SUPER,
    ] {
        assert_eq!(editor_tool_for_key(KeyCode::KeyS, modifiers), None);
    }
}

#[test]
fn brush_is_default_and_reselecting_it_is_a_no_op() {
    let mut input = PaintInputController::default();
    assert_eq!(input.tool(), PaintTool::Brush.into());
    assert!(!input.select_tool_for_key(KeyCode::KeyB));
    assert!(input.select_tool_for_key(KeyCode::KeyE));
    input.is_drawing = true;
    assert!(!input.select_tool_for_key(KeyCode::KeyB));
    assert_eq!(input.tool(), PaintTool::Eraser.into());
}

#[test]
fn transform_remembers_the_previous_paint_tool() {
    let mut input = PaintInputController::default();
    input.select_tool(PaintTool::Eraser.into());
    input.select_tool_for_key(KeyCode::KeyT);
    assert_eq!(input.previous_paint_tool(), PaintTool::Eraser.into());
    input.select_tool_for_key(KeyCode::KeyT);
    assert_eq!(input.tool(), PaintTool::Eraser.into());
}

#[test]
fn vertical_drag_resizes_and_clamps_brush() {
    let range = 10.0..=100.0;
    assert_eq!(resized_brush_size(50.0, 200.0, 180.0, range.clone()), 70.0);
    assert_eq!(resized_brush_size(50.0, 200.0, 230.0, range.clone()), 20.0);
    assert_eq!(resized_brush_size(50.0, 200.0, 100.0, range.clone()), 100.0);
    assert_eq!(resized_brush_size(50.0, 200.0, 300.0, range), 10.0);
}

#[test]
fn option_activates_eyedropper_before_dragging() {
    let input = PaintInputController {
        cursor_inside: true,
        cursor_pos: [40.0, 50.0],
        modifiers: ModifiersState::ALT,
        ..PaintInputController::default()
    };

    assert_eq!(input.brush_cursor_pos(), None);
    assert_eq!(input.eyedropper_indicator_pos(), Some([40.0, 50.0]));
    assert!(input.is_eyedropper_active());
    assert!(!input.has_active_document_drag());
}

#[test]
fn quick_menu_key_opens_on_press_and_reports_release() {
    let open = Some(QuickMenuRequest::Open {
        position: [3.0, 4.0],
        held_by_key: true,
    });
    let request = |state, repeat, modifiers, busy| {
        quick_menu_key_request(state, repeat, modifiers, busy, [3.0, 4.0])
    };

    assert_eq!(
        request(ElementState::Pressed, false, ModifiersState::empty(), false),
        open
    );
    assert_eq!(
        request(ElementState::Pressed, true, ModifiersState::empty(), false),
        None
    );
    assert_eq!(
        request(ElementState::Pressed, false, ModifiersState::SUPER, false),
        None
    );
    assert_eq!(
        request(ElementState::Pressed, false, ModifiersState::empty(), true),
        None
    );
    assert_eq!(
        request(
            ElementState::Released,
            false,
            ModifiersState::empty(),
            false
        ),
        Some(QuickMenuRequest::ReleaseKey)
    );
}

#[test]
fn active_document_interactions_capture_consumed_events() {
    let drawing = PaintInputController {
        is_drawing: true,
        ..PaintInputController::default()
    };
    let panning = PaintInputController {
        is_panning: true,
        ..PaintInputController::default()
    };

    assert!(drawing.has_active_document_drag());
    assert!(panning.has_active_document_drag());
    assert!(!PaintInputController::default().has_active_document_drag());
}

#[test]
fn shift_alone_keeps_the_brush_cursor_visible() {
    let input = PaintInputController {
        cursor_inside: true,
        cursor_pos: [40.0, 50.0],
        modifiers: ModifiersState::SHIFT,
        ..PaintInputController::default()
    };
    assert_eq!(input.brush_cursor_pos(), Some([40.0, 50.0]));
    assert_eq!(input.brush_resize_pos(), None);
    assert!(!input.is_resizing_brush());
}

#[test]
fn resize_session_keeps_outline_anchored() {
    let mut input = PaintInputController {
        cursor_inside: true,
        cursor_pos: [20.0, 30.0],
        ..PaintInputController::default()
    };
    input.begin_brush_resize_drag(48.0);

    assert_eq!(input.brush_cursor_pos(), None);
    assert_eq!(input.brush_resize_pos(), Some([20.0, 30.0]));
    assert!(input.is_resizing_brush());
    assert!(input.has_active_document_drag());

    input.resize_drag = None;
    input.cursor_pos = [40.0, 50.0];
    assert_eq!(input.brush_resize_pos(), Some([20.0, 30.0]));
    assert_eq!(input.brush_cursor_pos(), None);
    assert!(input.is_resizing_brush());

    input.resize_origin = None;
    assert_eq!(input.brush_cursor_pos(), Some([40.0, 50.0]));
}

#[test]
fn repeated_resize_drags_keep_the_session_origin() {
    let mut input = PaintInputController {
        cursor_pos: [20.0, 30.0],
        ..PaintInputController::default()
    };
    input.begin_brush_resize_drag(48.0);
    input.cursor_pos = [40.0, 80.0];
    input.begin_brush_resize_drag(64.0);

    assert_eq!(input.resize_origin, Some([20.0, 30.0]));
    let drag = input.resize_drag.unwrap();
    assert_eq!(drag.start_y, 80.0);
    assert_eq!(drag.start_size, 64.0);
}

#[test]
fn logical_application_shortcuts_follow_the_keyboard_layout() {
    assert_eq!(
        logical_shortcut_key(&Key::Character("z".into())),
        Some(KeyCode::KeyZ)
    );
    assert_eq!(
        logical_shortcut_key(&Key::Character("Y".into())),
        Some(KeyCode::KeyY)
    );
    assert_eq!(
        logical_shortcut_key(&Key::Character(",".into())),
        Some(KeyCode::Comma)
    );
    assert_eq!(
        logical_shortcut_key(&Key::Named(NamedKey::ArrowLeft)),
        Some(KeyCode::ArrowLeft)
    );
}

#[test]
fn maps_application_shortcuts() {
    assert_eq!(
        application_command_for_key(KeyCode::KeyN, ModifiersState::CONTROL),
        Some(AppCommand::Navigation(NavigationCommand::NewArtwork))
    );
    assert_eq!(
        application_command_for_key(KeyCode::Comma, ModifiersState::CONTROL),
        Some(AppCommand::Ui(UiCommand::ShowSettings))
    );
    assert_eq!(
        application_command_for_key(KeyCode::Slash, ModifiersState::SHIFT),
        Some(AppCommand::Ui(UiCommand::ShowShortcuts))
    );
    for modifiers in [
        ModifiersState::empty(),
        ModifiersState::CONTROL | ModifiersState::SHIFT,
        ModifiersState::CONTROL | ModifiersState::ALT,
    ] {
        assert_eq!(application_command_for_key(KeyCode::KeyN, modifiers), None);
    }
}

#[test]
fn maps_canvas_view_shortcuts() {
    assert_eq!(
        canvas_command_for_key(KeyCode::KeyR, ModifiersState::SHIFT),
        Some(AppCommand::Editor(EditorCommand::ResetCanvasRotation))
    );
    assert_eq!(
        canvas_command_for_key(KeyCode::KeyR, ModifiersState::empty()),
        None
    );
    let canvas_modifiers = ModifiersState::CONTROL | ModifiersState::ALT;
    assert_eq!(
        canvas_command_for_key(KeyCode::ArrowLeft, canvas_modifiers),
        Some(AppCommand::Editor(EditorCommand::RotateCanvasLeft))
    );
    assert_eq!(
        canvas_command_for_key(KeyCode::ArrowRight, canvas_modifiers),
        Some(AppCommand::Editor(EditorCommand::RotateCanvasRight))
    );
    assert_eq!(
        canvas_command_for_key(KeyCode::KeyC, canvas_modifiers),
        Some(AppCommand::Editor(EditorCommand::RequestCanvasResize))
    );
}

#[test]
fn maps_document_shortcuts() {
    assert_eq!(
        document_command_for_key(KeyCode::KeyS, ModifiersState::CONTROL),
        Some(AppCommand::Editor(EditorCommand::SaveArtwork))
    );
    assert_eq!(
        document_command_for_key(
            KeyCode::KeyE,
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        ),
        Some(AppCommand::Editor(EditorCommand::ExportPng))
    );
    assert_eq!(
        document_command_for_key(KeyCode::KeyG, ModifiersState::CONTROL),
        Some(AppCommand::Navigation(NavigationCommand::ShowGallery))
    );
    assert_eq!(
        document_command_for_key(
            KeyCode::KeyS,
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        ),
        None
    );
}

#[test]
fn pen_events_map_to_scaled_pointer_events() {
    let down = window_events_for_pen(
        PenEvent::Down {
            position: [10.0, 20.0],
            pressure: 0.5,
            time: Duration::from_millis(10),
        },
        2.0,
    );
    assert!(matches!(
        down.as_slice(),
        [
            WindowEvent::CursorMoved { position, .. },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            }
        ] if position.x == 20.0 && position.y == 40.0
    ));

    let motion = window_events_for_pen(
        PenEvent::Motion {
            position: [3.5, 7.25],
            pressure: 0.0,
            contact: false,
            time: Duration::from_millis(20),
        },
        1.0,
    );
    assert!(matches!(
        motion.as_slice(),
        [WindowEvent::CursorMoved { position, .. }]
            if position.x == 3.5 && position.y == 7.25
    ));

    assert!(matches!(
        window_events_for_pen(PenEvent::Up, 1.0).as_slice(),
        [WindowEvent::MouseInput {
            state: ElementState::Released,
            button: MouseButton::Left,
            ..
        }]
    ));
    assert!(matches!(
        window_events_for_pen(PenEvent::Leave, 1.0).as_slice(),
        [WindowEvent::CursorLeft { .. }]
    ));
}

#[test]
fn maps_non_macos_history_shortcuts() {
    assert_eq!(
        history_command_for_key(KeyCode::KeyZ, ModifiersState::CONTROL),
        Some(AppCommand::Editor(EditorCommand::Undo))
    );
    assert_eq!(
        history_command_for_key(
            KeyCode::KeyZ,
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        ),
        Some(AppCommand::Editor(EditorCommand::Redo))
    );
    assert_eq!(
        history_command_for_key(KeyCode::KeyY, ModifiersState::CONTROL),
        Some(AppCommand::Editor(EditorCommand::Redo))
    );
    assert_eq!(
        history_command_for_key(KeyCode::KeyZ, ModifiersState::SHIFT),
        None
    );
}
