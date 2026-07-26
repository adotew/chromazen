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

#[derive(Debug, Clone, Copy)]
struct BrushResizeDrag {
    start_y: f32,
    start_size: f32,
}

#[derive(Debug, Default)]
pub struct PaintInputController {
    cursor_pos: [f32; 2],
    cursor_inside: bool,
    is_drawing: bool,
    is_panning: bool,
    is_space_down: bool,
    is_resize_down: bool,
    resize_origin: Option<[f32; 2]>,
    resize_drag: Option<BrushResizeDrag>,
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

    pub fn brush_cursor_pos(&self) -> Option<[f32; 2]> {
        (self.cursor_inside && !self.is_panning && !self.is_space_down && !self.is_resize_down)
            .then_some(self.cursor_pos)
    }

    pub fn is_resizing_brush(&self) -> bool {
        self.is_resize_down
    }

    pub fn brush_resize_pos(&self) -> Option<[f32; 2]> {
        if !self.is_resize_down {
            return None;
        }
        self.resize_origin
            .or(self.cursor_inside.then_some(self.cursor_pos))
    }

    pub fn brush_resize_is_anchored(&self) -> bool {
        self.resize_origin.is_some()
    }

    pub fn captures_resize_event(&self, event: &WindowEvent) -> bool {
        self.resize_drag.is_some()
            || (self.is_resize_down
                && (matches!(event, WindowEvent::Focused(false))
                    || matches!(
                        event,
                        WindowEvent::KeyboardInput { event, .. }
                            if event.physical_key == PhysicalKey::Code(KeyCode::KeyR)
                    )))
    }

    pub fn observe_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let next = [position.x as f32, position.y as f32];
                let changed = !self.cursor_inside || self.cursor_pos != next;
                self.cursor_pos = next;
                self.cursor_inside = true;
                changed
            }
            WindowEvent::CursorLeft { .. } => std::mem::replace(&mut self.cursor_inside, false),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                false
            }
            WindowEvent::Focused(false) => {
                self.modifiers = ModifiersState::empty();
                std::mem::replace(&mut self.cursor_inside, false)
            }
            _ => false,
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
        brush: &mut BrushSettings,
        brush_size_range: std::ops::RangeInclusive<f32>,
        smoothing_options: StrokeSmoothingOptions,
        pressure_state: &PressureStateHandle,
    ) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let next = [position.x as f32, position.y as f32];

                if let Some(drag) = self.resize_drag {
                    let next_size = resized_brush_size(
                        drag.start_size,
                        drag.start_y,
                        next[1],
                        brush_size_range,
                    );
                    let changed = (brush.size - next_size).abs() > f32::EPSILON;
                    brush.size = next_size;
                    return changed;
                }
                if self.is_resize_down {
                    return false;
                }

                if self.is_panning {
                    let delta = [
                        next[0] - self.last_pan_pos[0],
                        next[1] - self.last_pan_pos[1],
                    ];
                    self.last_pan_pos = next;
                    if delta[0] != 0.0 || delta[1] != 0.0 {
                        paint.pan_by_window_delta(delta);
                        return true;
                    }
                    return false;
                }

                if self.is_drawing {
                    let point = self.stroke_point_from_window(paint, next, *brush, pressure_state);
                    let smoothed_points = self.smoother.push(point);
                    let queued = self.queue_smoothed_points(paint, smoothed_points, *brush);
                    return queued > 0;
                }

                true
            }
            WindowEvent::MouseInput { state, button, .. } if self.is_resize_down => {
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => {
                        self.begin_brush_resize_drag(brush.size);
                        true
                    }
                    (ElementState::Released, MouseButton::Left) => {
                        self.resize_drag.take().is_some()
                    }
                    _ => false,
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    true
                }
                (ElementState::Pressed, MouseButton::Left) => {
                    if !paint.can_paint() {
                        return false;
                    }
                    let point = self.stroke_point_from_window(
                        paint,
                        self.cursor_pos,
                        *brush,
                        pressure_state,
                    );
                    if !paint.begin_stroke(self.tool, point, brush.rgba()) {
                        return false;
                    }
                    self.is_drawing = true;
                    self.last_point = Some(point);
                    self.smoothing_options = smoothing_options;
                    self.smoother
                        .begin_with_strength(point, smoothing_options.strength);
                    self.tool != PaintTool::Smudge && paint.queue_stamp(point)
                }
                (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    true
                }
                (ElementState::Released, _) => self.end_stroke(paint, *brush),
                _ => false,
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
                    return (paint.zoom() - old_zoom).abs() > f32::EPSILON;
                }
                false
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyR) {
                    if event.repeat {
                        return false;
                    }
                    let resize_down = event.state == ElementState::Pressed
                        && !self.modifiers.control_key()
                        && !self.modifiers.alt_key()
                        && !self.modifiers.super_key();
                    let changed = self.is_resize_down != resize_down;
                    let ended_stroke = if resize_down && changed {
                        self.end_stroke(paint, *brush)
                    } else {
                        false
                    };
                    self.is_resize_down = resize_down;
                    if !resize_down {
                        self.resize_origin = None;
                        self.resize_drag = None;
                    }
                    return changed || ended_stroke;
                }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && let PhysicalKey::Code(key) = event.physical_key
                    && self.select_tool_for_key(key)
                {
                    return true;
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) {
                    let is_space_down = event.state == ElementState::Pressed;
                    let changed = self.is_space_down != is_space_down;
                    self.is_space_down = is_space_down;
                    if !self.is_space_down {
                        self.is_panning = false;
                    }
                    return changed;
                }
                false
            }
            WindowEvent::CursorLeft { .. } => {
                self.resize_drag = None;
                self.end_stroke(paint, *brush)
            }
            WindowEvent::Focused(false) => {
                self.is_resize_down = false;
                self.resize_origin = None;
                self.resize_drag = None;
                self.end_stroke(paint, *brush)
            }
            _ => false,
        }
    }

    fn begin_brush_resize_drag(&mut self, brush_size: f32) {
        self.resize_origin.get_or_insert(self.cursor_pos);
        self.resize_drag = Some(BrushResizeDrag {
            start_y: self.cursor_pos[1],
            start_size: brush_size,
        });
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
        let mut queued = 0;
        for point in points {
            if let Some(previous) = self.last_point {
                queued += paint.stamp_line(previous, point, brush.spacing);
            } else if paint.queue_stamp(point) {
                queued += 1;
            }
            self.last_point = Some(point);
        }
        queued
    }

    fn end_stroke(&mut self, paint: &mut PaintRenderer, brush: BrushSettings) -> bool {
        let was_active = self.is_drawing || self.is_panning;
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
        queued > 0 || was_active
    }
}

fn resized_brush_size(
    start_size: f32,
    start_y: f32,
    current_y: f32,
    range: std::ops::RangeInclusive<f32>,
) -> f32 {
    (start_size + start_y - current_y).clamp(*range.start(), *range.end())
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
    fn vertical_drag_resizes_and_clamps_brush() {
        let range = 10.0..=100.0;
        assert_eq!(resized_brush_size(50.0, 200.0, 180.0, range.clone()), 70.0);
        assert_eq!(resized_brush_size(50.0, 200.0, 230.0, range.clone()), 20.0);
        assert_eq!(resized_brush_size(50.0, 200.0, 100.0, range.clone()), 100.0);
        assert_eq!(resized_brush_size(50.0, 200.0, 300.0, range), 10.0);
    }

    #[test]
    fn resizing_hides_brush_cursor() {
        let mut input = PaintInputController {
            cursor_inside: true,
            is_resize_down: true,
            ..PaintInputController::default()
        };
        assert_eq!(input.brush_cursor_pos(), None);
        assert_eq!(input.brush_resize_pos(), Some([0.0, 0.0]));
        assert!(input.captures_resize_event(&WindowEvent::Focused(false)));
        input.resize_origin = Some([20.0, 30.0]);
        input.cursor_pos = [40.0, 50.0];
        assert_eq!(input.brush_resize_pos(), Some([20.0, 30.0]));
        assert!(input.brush_resize_is_anchored());
        input.is_resize_down = false;
        assert_eq!(input.brush_cursor_pos(), Some([40.0, 50.0]));
    }

    #[test]
    fn repeated_resize_drags_keep_the_first_origin() {
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
