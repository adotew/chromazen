use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

use crate::{
    brush::{BrushSettings, StrokePoint},
    macos_pressure::PressureStateHandle,
    renderer::PaintRenderer,
};

#[derive(Debug, Default)]
pub struct PaintInputController {
    cursor_pos: [f32; 2],
    is_drawing: bool,
    is_panning: bool,
    is_space_down: bool,
    last_point: Option<StrokePoint>,
    last_pan_pos: [f32; 2],
}

impl PaintInputController {
    pub fn handle_event(
        &mut self,
        event: &WindowEvent,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
        pressure_state: &PressureStateHandle,
    ) -> bool {
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
                        return true;
                    }
                    return false;
                }

                if self.is_drawing {
                    let point = self.stroke_point_from_window(paint, next, brush, pressure_state);
                    let queued = if let Some(previous) = self.last_point {
                        paint.stamp_line(previous, point, brush.rgba())
                    } else {
                        0
                    };
                    self.last_point = Some(point);
                    return queued > 0;
                }

                false
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    false
                }
                (ElementState::Pressed, MouseButton::Left) => {
                    let point = self.stroke_point_from_window(
                        paint,
                        self.cursor_pos,
                        brush,
                        pressure_state,
                    );
                    self.is_drawing = true;
                    self.last_point = Some(point);
                    paint.begin_stroke();
                    paint.queue_stamp(point, brush.rgba())
                }
                (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    false
                }
                (ElementState::Released, _) => {
                    self.end_stroke(paint);
                    false
                }
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
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) {
                    self.is_space_down = event.state == ElementState::Pressed;
                    if !self.is_space_down {
                        self.is_panning = false;
                    }
                }
                false
            }
            WindowEvent::CursorLeft { .. } => {
                self.end_stroke(paint);
                false
            }
            _ => false,
        }
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

    fn end_stroke(&mut self, paint: &mut PaintRenderer) {
        if self.is_drawing {
            paint.end_stroke();
        }
        self.is_drawing = false;
        self.is_panning = false;
        self.last_point = None;
    }
}
