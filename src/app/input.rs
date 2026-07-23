use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
};

use crate::{
    paint::{BrushSettings, PaintTool, StrokePoint, StrokeSmoother, StrokeSmoothingOptions},
    platform::PressureStateHandle,
    renderer::PaintRenderer,
};

use super::command::AppCommand;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputOutcome {
    pub(crate) needs_redraw: bool,
    pub(crate) queued_stamps: usize,
    pub(crate) pressure_sampled: bool,
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
        }
    }
}

#[derive(Debug, Default)]
pub struct PaintInputController {
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

impl PaintInputController {
    pub fn tool(&self) -> PaintTool {
        self.tool
    }

    pub fn observe_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Focused(false) => self.modifiers = ModifiersState::empty(),
            _ => {}
        }
    }

    pub fn history_command(&self, event: &WindowEvent) -> Option<AppCommand> {
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            return None;
        }
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return None;
        };
        if event.state != ElementState::Pressed || event.repeat {
            return None;
        }
        let PhysicalKey::Code(key) = event.physical_key else {
            return None;
        };
        history_command_for_key(key, self.modifiers)
    }

    pub fn handle_event(
        &mut self,
        event: &WindowEvent,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
        smoothing_options: StrokeSmoothingOptions,
        pressure_state: &PressureStateHandle,
    ) -> InputOutcome {
        match event {
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
                    let point = self.stroke_point_from_window(paint, next, brush, pressure_state);
                    let smoothed_points = self.smoother.push(point);
                    let queued = self.queue_smoothed_points(paint, smoothed_points, brush);
                    return InputOutcome::stamps(queued);
                }

                InputOutcome::default()
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    InputOutcome::default()
                }
                (ElementState::Pressed, MouseButton::Left) => {
                    if !paint.can_paint() {
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
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    InputOutcome::default()
                }
                (ElementState::Released, _) => self.end_stroke(paint, brush),
                _ => InputOutcome::default(),
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) / 120.0,
                };
                if scroll != 0.0 {
                    let old_zoom = paint.zoom();
                    let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
                    paint.apply_zoom_at(factor, self.cursor_pos);
                    return if (paint.zoom() - old_zoom).abs() > f32::EPSILON {
                        InputOutcome::redraw()
                    } else {
                        InputOutcome::default()
                    };
                }
                InputOutcome::default()
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && let PhysicalKey::Code(key) = event.physical_key
                    && self.select_tool_for_key(key)
                {
                    return InputOutcome::redraw();
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) {
                    self.is_space_down = event.state == ElementState::Pressed;
                    if !self.is_space_down {
                        self.is_panning = false;
                    }
                }
                InputOutcome::default()
            }
            WindowEvent::CursorLeft { .. } | WindowEvent::Focused(false) => {
                self.end_stroke(paint, brush)
            }
            _ => InputOutcome::default(),
        }
    }

    fn select_tool_for_key(&mut self, key: KeyCode) -> bool {
        if self.is_drawing {
            return false;
        }
        let Some(tool) = paint_tool_for_key(key, self.modifiers) else {
            return false;
        };
        let changed = self.tool != tool;
        self.tool = tool;
        changed
    }

    fn stroke_point_from_window(
        &self,
        paint: &PaintRenderer,
        window_point: [f32; 2],
        brush: BrushSettings,
        pressure_state: &PressureStateHandle,
    ) -> StrokePoint {
        let doc = paint.window_to_document(window_point);
        brush.stroke_point(doc, pressure_state.brush_pressure())
    }

    fn queue_smoothed_points(
        &mut self,
        paint: &mut PaintRenderer,
        points: Vec<StrokePoint>,
        brush: BrushSettings,
    ) -> usize {
        let color = brush.rgba();
        let mut queued = 0;
        for point in points {
            if let Some(previous) = self.last_point {
                queued += paint.stamp_line(previous, point, color, brush.spacing);
            } else if paint.queue_stamp(point, color) {
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

fn history_command_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<AppCommand> {
    if !modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match (key, modifiers.shift_key()) {
        (KeyCode::KeyZ, false) => Some(AppCommand::Undo),
        (KeyCode::KeyZ, true) | (KeyCode::KeyY, false) => Some(AppCommand::Redo),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_tool_shortcuts() {
        assert_eq!(
            paint_tool_for_key(KeyCode::KeyB, ModifiersState::empty()),
            Some(PaintTool::Brush)
        );
        assert_eq!(
            paint_tool_for_key(KeyCode::KeyE, ModifiersState::SHIFT),
            Some(PaintTool::Eraser)
        );
        assert_eq!(
            paint_tool_for_key(KeyCode::KeyS, ModifiersState::empty()),
            Some(PaintTool::Smudge)
        );
        for modifiers in [
            ModifiersState::CONTROL,
            ModifiersState::ALT,
            ModifiersState::SUPER,
        ] {
            assert_eq!(paint_tool_for_key(KeyCode::KeyS, modifiers), None);
        }
    }

    #[test]
    fn brush_is_default_and_reselecting_it_is_a_no_op() {
        let mut input = PaintInputController::default();
        assert_eq!(input.tool(), PaintTool::Brush);
        assert!(!input.select_tool_for_key(KeyCode::KeyB));
        assert!(input.select_tool_for_key(KeyCode::KeyE));
        input.is_drawing = true;
        assert!(!input.select_tool_for_key(KeyCode::KeyB));
        assert_eq!(input.tool(), PaintTool::Eraser);
    }

    #[test]
    fn maps_linux_history_shortcuts() {
        assert_eq!(
            history_command_for_key(KeyCode::KeyZ, ModifiersState::CONTROL),
            Some(AppCommand::Undo)
        );
        assert_eq!(
            history_command_for_key(
                KeyCode::KeyZ,
                ModifiersState::CONTROL | ModifiersState::SHIFT,
            ),
            Some(AppCommand::Redo)
        );
        assert_eq!(
            history_command_for_key(KeyCode::KeyY, ModifiersState::CONTROL),
            Some(AppCommand::Redo)
        );
        assert_eq!(
            history_command_for_key(KeyCode::KeyZ, ModifiersState::SHIFT),
            None
        );
    }
}
