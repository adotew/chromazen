use chromazen::{
    paint::{BrushSettings, PaintTool, StrokePoint, StrokeSmoother, StrokeSmoothingOptions},
    platform::PressureStateHandle,
    renderer::PaintRenderer,
};
use tauri_runtime_wry::tao::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, ModifiersState},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputAction {
    Undo,
    Redo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputOutcome {
    pub(crate) needs_redraw: bool,
    pub(crate) queued_stamps: usize,
    pub(crate) pressure_sampled: bool,
    pub(crate) action: Option<InputAction>,
}

impl InputOutcome {
    fn redraw() -> Self {
        Self {
            needs_redraw: true,
            ..Self::default()
        }
    }

    fn stamps(queued_stamps: usize) -> Self {
        Self {
            needs_redraw: queued_stamps > 0,
            queued_stamps,
            pressure_sampled: queued_stamps > 0,
            action: None,
        }
    }

    fn action(action: InputAction) -> Self {
        Self {
            needs_redraw: true,
            action: Some(action),
            ..Self::default()
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct NativeInputController {
    cursor_pos: [f32; 2],
    is_drawing: bool,
    is_panning: bool,
    is_space_down: bool,
    last_point: Option<StrokePoint>,
    last_pan_pos: [f32; 2],
    smoother: StrokeSmoother,
    smoothing_options: StrokeSmoothingOptions,
    modifiers: ModifiersState,
    tool: PaintTool,
}

impl NativeInputController {
    pub(crate) fn tool(&self) -> PaintTool {
        self.tool
    }

    pub(crate) fn set_tool(&mut self, tool: PaintTool) -> bool {
        if self.is_drawing || self.tool == tool {
            return false;
        }
        self.tool = tool;
        true
    }

    pub(crate) fn handle_event(
        &mut self,
        event: &WindowEvent<'_>,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
        smoothing_options: StrokeSmoothingOptions,
        pressure_state: &PressureStateHandle,
    ) -> InputOutcome {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
                InputOutcome::default()
            }
            WindowEvent::CursorMoved { position, .. } => {
                let next = [position.x as f32, position.y as f32];
                self.cursor_pos = next;

                if self.is_panning {
                    let delta = [
                        next[0] - self.last_pan_pos[0],
                        next[1] - self.last_pan_pos[1],
                    ];
                    self.last_pan_pos = next;
                    if delta[0] != 0.0 || delta[1] != 0.0 {
                        paint.pan_by_window_delta(delta);
                        return InputOutcome::redraw();
                    }
                    return InputOutcome::default();
                }

                if self.is_drawing {
                    if !self.cursor_is_on_canvas(paint) {
                        return self.end_stroke(paint, brush);
                    }
                    let point = self.stroke_point_from_window(paint, next, brush, pressure_state);
                    let smoothed_points = self.smoother.push(point);
                    let queued = self.queue_smoothed_points(paint, smoothed_points, brush);
                    return InputOutcome::stamps(queued);
                }

                InputOutcome::default()
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    if self.cursor_is_on_canvas(paint) {
                        self.is_panning = true;
                        self.last_pan_pos = self.cursor_pos;
                    }
                    InputOutcome::default()
                }
                (ElementState::Pressed, MouseButton::Left) => {
                    if !paint.can_paint() || !self.cursor_is_on_canvas(paint) {
                        return InputOutcome::default();
                    }
                    let point = self.stroke_point_from_window(
                        paint,
                        self.cursor_pos,
                        brush,
                        pressure_state,
                    );
                    self.is_drawing = true;
                    self.last_point = Some(point);
                    self.smoothing_options = smoothing_options;
                    self.smoother
                        .begin_with_strength(point, smoothing_options.strength);
                    paint.begin_stroke(self.tool, point);
                    let queued = usize::from(
                        self.tool != PaintTool::Smudge && paint.queue_stamp(point, brush.rgba()),
                    );
                    InputOutcome::stamps(queued)
                }
                (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) => {
                    if self.cursor_is_on_canvas(paint) {
                        self.is_panning = true;
                        self.last_pan_pos = self.cursor_pos;
                    }
                    InputOutcome::default()
                }
                (ElementState::Released, _) => self.end_stroke(paint, brush),
                _ => InputOutcome::default(),
            },
            WindowEvent::MouseWheel { delta, .. } if self.cursor_is_on_canvas(paint) => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) / 120.0,
                    _ => 0.0,
                };
                if scroll == 0.0 {
                    return InputOutcome::default();
                }
                let old_zoom = paint.zoom();
                paint.apply_zoom_at(if scroll > 0.0 { 1.1 } else { 0.9 }, self.cursor_pos);
                if (paint.zoom() - old_zoom).abs() > f32::EPSILON {
                    InputOutcome::redraw()
                } else {
                    InputOutcome::default()
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat {
                    if let Some(action) = history_action_for_key(event.physical_key, self.modifiers)
                    {
                        return InputOutcome::action(action);
                    }
                    if let Some(tool) = paint_tool_for_key(event.physical_key, self.modifiers)
                        && self.set_tool(tool)
                    {
                        return InputOutcome::redraw();
                    }
                }
                if event.physical_key == KeyCode::Space {
                    self.is_space_down = event.state == ElementState::Pressed;
                    if !self.is_space_down {
                        self.is_panning = false;
                    }
                }
                InputOutcome::default()
            }
            WindowEvent::CursorLeft { .. } => self.end_stroke(paint, brush),
            WindowEvent::Focused(false) => {
                self.modifiers = ModifiersState::empty();
                self.is_space_down = false;
                self.end_stroke(paint, brush)
            }
            _ => InputOutcome::default(),
        }
    }

    fn cursor_is_on_canvas(&self, paint: &PaintRenderer) -> bool {
        self.cursor_pos[0] >= 0.0
            && self.cursor_pos[1] >= 0.0
            && self.cursor_pos[0] < paint.canvas_viewport_size()[0] as f32
            && self.cursor_pos[1] < paint.canvas_viewport_size()[1] as f32
    }

    fn stroke_point_from_window(
        &self,
        paint: &PaintRenderer,
        window_point: [f32; 2],
        brush: BrushSettings,
        pressure_state: &PressureStateHandle,
    ) -> StrokePoint {
        let document_point = paint.window_to_document(window_point);
        brush.stroke_point(document_point, pressure_state.brush_pressure())
    }

    fn queue_smoothed_points(
        &mut self,
        paint: &mut PaintRenderer,
        points: Vec<StrokePoint>,
        brush: BrushSettings,
    ) -> usize {
        let mut queued = 0;
        for point in points {
            if let Some(previous) = self.last_point {
                queued += paint.stamp_line(previous, point, brush.rgba(), brush.spacing);
            } else if paint.queue_stamp(point, brush.rgba()) {
                queued += 1;
            }
            self.last_point = Some(point);
        }
        queued
    }

    fn end_stroke(&mut self, paint: &mut PaintRenderer, brush: BrushSettings) -> InputOutcome {
        let queued = if self.is_drawing {
            let smoothed_points = self.smoother.finish();
            let queued = self.queue_smoothed_points(paint, smoothed_points, brush);
            paint.end_stroke();
            queued
        } else {
            self.smoother.reset();
            0
        };
        self.is_drawing = false;
        self.is_panning = false;
        self.last_point = None;
        InputOutcome::stamps(queued)
    }
}

fn paint_tool_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<PaintTool> {
    if modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match key {
        KeyCode::KeyB => Some(PaintTool::Brush),
        KeyCode::KeyE => Some(PaintTool::Eraser),
        KeyCode::KeyS => Some(PaintTool::Smudge),
        _ => None,
    }
}

fn history_action_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<InputAction> {
    let command_modifier = if cfg!(target_os = "macos") {
        modifiers.super_key()
    } else {
        modifiers.control_key()
    };
    if !command_modifier || modifiers.alt_key() {
        return None;
    }
    match (key, modifiers.shift_key()) {
        (KeyCode::KeyZ, false) => Some(InputAction::Undo),
        (KeyCode::KeyZ, true) | (KeyCode::KeyY, false) => Some(InputAction::Redo),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_native_tool_shortcuts() {
        assert_eq!(
            paint_tool_for_key(KeyCode::KeyB, ModifiersState::empty()),
            Some(PaintTool::Brush)
        );
        assert_eq!(
            paint_tool_for_key(KeyCode::KeyE, ModifiersState::SHIFT),
            Some(PaintTool::Eraser)
        );
        assert_eq!(
            paint_tool_for_key(KeyCode::KeyS, ModifiersState::CONTROL),
            None
        );
    }

    #[test]
    fn maps_platform_history_shortcuts() {
        let modifier = if cfg!(target_os = "macos") {
            ModifiersState::SUPER
        } else {
            ModifiersState::CONTROL
        };
        assert_eq!(
            history_action_for_key(KeyCode::KeyZ, modifier),
            Some(InputAction::Undo)
        );
        assert_eq!(
            history_action_for_key(KeyCode::KeyZ, modifier | ModifiersState::SHIFT),
            Some(InputAction::Redo)
        );
    }
}
