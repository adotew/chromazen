use super::*;

#[test]
fn top_controls_select_cancel_and_apply_tools() {
    for (x, active_tool, has_selection, expected_tool, expected_command) in [
        (
            20.0,
            EditorTool::Paint(PaintTool::Brush),
            false,
            Some(EditorTool::Select),
            None,
        ),
        (
            60.0,
            EditorTool::Paint(PaintTool::Brush),
            false,
            Some(EditorTool::Transform),
            None,
        ),
        (
            20.0,
            EditorTool::Select,
            false,
            None,
            Some(EditorCommand::CancelSelection),
        ),
        (
            20.0,
            EditorTool::Paint(PaintTool::Brush),
            true,
            None,
            Some(EditorCommand::CancelSelection),
        ),
        (
            60.0,
            EditorTool::Transform,
            false,
            None,
            Some(EditorCommand::ApplyLayerTransform),
        ),
    ] {
        let context = egui::Context::default();
        let mut commands = Vec::new();
        let mut selected_tool = None;
        let position = egui::pos2(x, 16.0);
        for pressed in [None, Some(true), Some(false)] {
            let mut events = vec![egui::Event::PointerMoved(position)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = context.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    selected_tool = show_selection_tools(
                        ui,
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(80.0, 32.0)),
                        active_tool,
                        has_selection,
                        &mut commands,
                    );
                },
            );
        }
        assert_eq!(selected_tool, expected_tool);
        assert_eq!(commands.len(), usize::from(expected_command.is_some()));
        if let Some(expected) = expected_command {
            assert_eq!(commands, vec![AppCommand::Editor(expected)]);
        }
    }
}
