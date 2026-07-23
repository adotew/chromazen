use chromazen::{
    paint::{BrushSettings, PaintTool, StrokePoint, StrokeSmoother, StrokeSmoothingOptions},
    platform::PressureStateHandle,
    renderer::PaintRenderer,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerButton {
    Left,
    Middle,
    Right,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanvasKey {
    B,
    E,
    S,
    Space,
    Y,
    Z,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyModifiers {
    pub(crate) control: bool,
    pub(crate) alt: bool,
    pub(crate) super_key: bool,
    pub(crate) shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CanvasEvent {
    ModifiersChanged(KeyModifiers),
    CursorMoved([f32; 2]),
    Pointer {
        state: ButtonState,
        button: PointerButton,
    },
    Scroll(f32),
    KeyInput {
        state: ButtonState,
        key: CanvasKey,
        repeat: bool,
    },
    CursorLeft,
    FocusLost,
}

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
    modifiers: KeyModifiers,
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
        event: &CanvasEvent,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
        smoothing_options: StrokeSmoothingOptions,
        pressure_state: &PressureStateHandle,
    ) -> InputOutcome {
        match event {
            CanvasEvent::ModifiersChanged(modifiers) => {
                self.modifiers = *modifiers;
                InputOutcome::default()
            }
            CanvasEvent::CursorMoved(next) => {
                self.cursor_pos = *next;

                if self.is_panning {
                    let delta = [
                        next[0] - self.last_pan_pos[0],
                        next[1] - self.last_pan_pos[1],
                    ];
                    self.last_pan_pos = *next;
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
                    let point = self.stroke_point_from_window(paint, *next, brush, pressure_state);
                    let smoothed_points = self.smoother.push(point);
                    let queued = self.queue_smoothed_points(paint, smoothed_points, brush);
                    return InputOutcome::stamps(queued);
                }

                InputOutcome::default()
            }
            CanvasEvent::Pointer { state, button } => match (*state, *button) {
                (ButtonState::Pressed, PointerButton::Left) if self.is_space_down => {
                    if self.cursor_is_on_canvas(paint) {
                        self.is_panning = true;
                        self.last_pan_pos = self.cursor_pos;
                    }
                    InputOutcome::default()
                }
                (ButtonState::Pressed, PointerButton::Left) => {
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
                (ButtonState::Pressed, PointerButton::Middle | PointerButton::Right) => {
                    if self.cursor_is_on_canvas(paint) {
                        self.is_panning = true;
                        self.last_pan_pos = self.cursor_pos;
                    }
                    InputOutcome::default()
                }
                (ButtonState::Released, PointerButton::Left) => {
                    let outcome = self.end_stroke(paint, brush);
                    pressure_state.clear_pen();
                    outcome
                }
                (ButtonState::Released, _) => self.end_stroke(paint, brush),
                _ => InputOutcome::default(),
            },
            CanvasEvent::Scroll(scroll) if self.cursor_is_on_canvas(paint) => {
                if *scroll == 0.0 {
                    return InputOutcome::default();
                }
                let old_zoom = paint.zoom();
                paint.apply_zoom_at(if *scroll > 0.0 { 1.1 } else { 0.9 }, self.cursor_pos);
                if (paint.zoom() - old_zoom).abs() > f32::EPSILON {
                    InputOutcome::redraw()
                } else {
                    InputOutcome::default()
                }
            }
            CanvasEvent::KeyInput { state, key, repeat } => {
                if *state == ButtonState::Pressed && !repeat {
                    if let Some(action) = history_action_for_key(*key, self.modifiers) {
                        return InputOutcome::action(action);
                    }
                    if let Some(tool) = paint_tool_for_key(*key, self.modifiers)
                        && self.set_tool(tool)
                    {
                        return InputOutcome::redraw();
                    }
                }
                if *key == CanvasKey::Space {
                    self.is_space_down = *state == ButtonState::Pressed;
                    if !self.is_space_down {
                        self.is_panning = false;
                    }
                }
                InputOutcome::default()
            }
            CanvasEvent::CursorLeft => self.end_stroke(paint, brush),
            CanvasEvent::FocusLost => {
                self.modifiers = KeyModifiers::default();
                self.is_space_down = false;
                self.end_stroke(paint, brush)
            }
            CanvasEvent::Scroll(_) => InputOutcome::default(),
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

fn paint_tool_for_key(key: CanvasKey, modifiers: KeyModifiers) -> Option<PaintTool> {
    if modifiers.control || modifiers.alt || modifiers.super_key {
        return None;
    }
    match key {
        CanvasKey::B => Some(PaintTool::Brush),
        CanvasKey::E => Some(PaintTool::Eraser),
        CanvasKey::S => Some(PaintTool::Smudge),
        _ => None,
    }
}

fn history_action_for_key(key: CanvasKey, modifiers: KeyModifiers) -> Option<InputAction> {
    let command_modifier = if cfg!(target_os = "macos") {
        modifiers.super_key
    } else {
        modifiers.control
    };
    if !command_modifier || modifiers.alt {
        return None;
    }
    match (key, modifiers.shift) {
        (CanvasKey::Z, false) => Some(InputAction::Undo),
        (CanvasKey::Z, true) | (CanvasKey::Y, false) => Some(InputAction::Redo),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_native_tool_shortcuts() {
        assert_eq!(
            paint_tool_for_key(CanvasKey::B, KeyModifiers::default()),
            Some(PaintTool::Brush)
        );
        assert_eq!(
            paint_tool_for_key(
                CanvasKey::E,
                KeyModifiers {
                    shift: true,
                    ..KeyModifiers::default()
                },
            ),
            Some(PaintTool::Eraser)
        );
        assert_eq!(
            paint_tool_for_key(
                CanvasKey::S,
                KeyModifiers {
                    control: true,
                    ..KeyModifiers::default()
                },
            ),
            None
        );
    }

    #[test]
    fn maps_platform_history_shortcuts() {
        let mut modifiers = KeyModifiers::default();
        if cfg!(target_os = "macos") {
            modifiers.super_key = true;
        } else {
            modifiers.control = true;
        }
        assert_eq!(
            history_action_for_key(CanvasKey::Z, modifiers),
            Some(InputAction::Undo)
        );
        modifiers.shift = true;
        assert_eq!(
            history_action_for_key(CanvasKey::Z, modifiers),
            Some(InputAction::Redo)
        );
    }
}
