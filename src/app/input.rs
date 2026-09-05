use std::time::{Duration, Instant};

use winit::{
    dpi::PhysicalPosition,
    event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
};

use crate::{
    paint::{BrushSettings, PaintTool, StrokePoint, StrokePositionFilter, StrokeSmoother},
    platform::{PenEvent, PressureStateHandle},
    renderer::PaintRenderer,
};

use super::command::{AppCommand, EditorCommand, NavigationCommand, UiCommand};

const EYEDROPPER_DRAG_SAMPLE_INTERVAL: Duration = Duration::from_millis(33);
const ROTATION_SNAP_INTERVAL: f32 = std::f32::consts::FRAC_PI_2;
const ROTATION_SNAP_ENTER: f32 = 5.0_f32.to_radians();
const ROTATION_SNAP_EXIT: f32 = 8.0_f32.to_radians();

/// Converts tablet input to the same physical-coordinate events used by mouse input.
pub(crate) fn window_events_for_pen(event: PenEvent, scale_factor: f64) -> Vec<WindowEvent> {
    fn cursor_moved(position: [f32; 2], scale_factor: f64) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(
                f64::from(position[0]) * scale_factor,
                f64::from(position[1]) * scale_factor,
            ),
        }
    }

    match event {
        PenEvent::Motion { position, .. } => vec![cursor_moved(position, scale_factor)],
        PenEvent::Down { position, .. } => vec![
            cursor_moved(position, scale_factor),
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
            },
        ],
        PenEvent::Up => vec![WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
        }],
        PenEvent::Leave => vec![WindowEvent::CursorLeft {
            device_id: DeviceId::dummy(),
        }],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrushAdjustment {
    Size,
    Opacity,
}

#[derive(Debug, Clone, Copy)]
struct BrushResizeDrag {
    start_x: f32,
    start_y: f32,
    start_size: f32,
    start_opacity: f32,
    adjustment: Option<BrushAdjustment>,
}

impl BrushResizeDrag {
    fn resolve_adjustment_axis(&mut self, point: [f32; 2]) -> Option<BrushAdjustment> {
        if self.adjustment.is_none() {
            let delta_x = (point[0] - self.start_x).abs();
            let delta_y = (point[1] - self.start_y).abs();
            if delta_x == 0.0 && delta_y == 0.0 {
                return None;
            }
            self.adjustment = Some(if delta_x > delta_y {
                BrushAdjustment::Opacity
            } else {
                BrushAdjustment::Size
            });
        }
        self.adjustment
    }
}

#[derive(Debug, Clone, Copy)]
struct RotationDrag {
    anchor: [f32; 2],
    start_rotation: f32,
    start_pointer_angle: f32,
    snapped_to: Option<f32>,
}

#[derive(Debug, Default)]
struct EyedropperDrag {
    last_sample: Option<(Instant, [f32; 2])>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyboardShortcut {
    ToggleSidebar,
    CycleTool,
    FlipCanvasHorizontal,
    FlipCanvasVertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorTool {
    Paint(PaintTool),
    Transform,
}

impl Default for EditorTool {
    fn default() -> Self {
        Self::Paint(PaintTool::default())
    }
}

impl EditorTool {
    pub(crate) fn paint_tool(self) -> Option<PaintTool> {
        match self {
            Self::Paint(tool) => Some(tool),
            Self::Transform => None,
        }
    }
}

impl From<PaintTool> for EditorTool {
    fn from(tool: PaintTool) -> Self {
        Self::Paint(tool)
    }
}

#[derive(Debug, Default)]
pub struct PaintInputController {
    cursor_pos: [f32; 2],
    cursor_inside: bool,
    is_drawing: bool,
    stroke_uses_pen_pressure: bool,
    is_panning: bool,
    is_space_down: bool,
    is_rotation_key_down: bool,
    rotation_drag: Option<RotationDrag>,
    resize_origin: Option<[f32; 2]>,
    resize_drag: Option<BrushResizeDrag>,
    eyedropper_drag: Option<EyedropperDrag>,
    last_point: Option<StrokePoint>,
    last_raw_point: Option<StrokePoint>,
    last_pan_pos: [f32; 2],
    position_filter: StrokePositionFilter,
    smoother: StrokeSmoother,
    input_clock: Option<Instant>,
    scale_factor: Option<f32>,
    modifiers: ModifiersState,
    tool: EditorTool,
    previous_paint_tool: PaintTool,
}

impl PaintInputController {
    pub(crate) fn set_scale_factor(&mut self, scale_factor: f32) {
        self.scale_factor = Some(scale_factor.max(f32::EPSILON));
    }

    pub fn tool(&self) -> EditorTool {
        self.tool
    }

    pub(crate) fn previous_paint_tool(&self) -> EditorTool {
        self.previous_paint_tool.into()
    }

    pub(crate) fn cursor_position(&self) -> [f32; 2] {
        self.cursor_pos
    }

    pub fn brush_cursor_pos(&self) -> Option<[f32; 2]> {
        (self.cursor_inside
            && !self.is_panning
            && !self.is_space_down
            && !self.is_rotation_key_down
            && self.resize_origin.is_none()
            && !self.is_eyedropper_active()
            && self.tool.paint_tool().is_some())
        .then_some(self.cursor_pos)
    }

    pub fn eyedropper_indicator_pos(&self) -> Option<[f32; 2]> {
        (self.cursor_inside && self.is_eyedropper_active()).then_some(self.cursor_pos)
    }

    pub fn is_eyedropper_active(&self) -> bool {
        eyedropper_modifier_is_active(self.modifiers)
    }

    pub fn is_resizing_brush(&self) -> bool {
        self.resize_origin.is_some()
    }

    pub fn is_panning(&self) -> bool {
        self.is_panning
    }

    pub(crate) fn is_rotating_canvas(&self) -> bool {
        self.rotation_drag.is_some()
    }

    pub fn is_pan_modifier_active(&self) -> bool {
        self.is_space_down
    }

    pub fn brush_resize_pos(&self) -> Option<[f32; 2]> {
        self.resize_origin
    }

    pub fn brush_resize_is_anchored(&self) -> bool {
        self.resize_origin.is_some()
    }

    pub fn has_active_document_drag(&self) -> bool {
        self.is_drawing
            || self.is_panning
            || self.resize_origin.is_some()
            || self.rotation_drag.is_some()
            || self.eyedropper_drag.is_some()
    }

    pub fn update_pointer_and_modifier_state(&mut self, event: &WindowEvent) -> bool {
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
                let was_active = self.is_eyedropper_active();
                self.modifiers = modifiers.state();
                was_active != self.is_eyedropper_active()
            }
            WindowEvent::Focused(false) => {
                let changed = self.cursor_inside
                    || self.is_eyedropper_active()
                    || self.eyedropper_drag.is_some();
                self.modifiers = ModifiersState::empty();
                self.cursor_inside = false;
                changed
            }
            _ => false,
        }
    }

    pub fn keyboard_shortcut(&self, event: &WindowEvent) -> Option<KeyboardShortcut> {
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return None;
        };
        let PhysicalKey::Code(key) = event.physical_key else {
            return None;
        };
        keyboard_shortcut_for_key(key, event.state, event.repeat, self.modifiers)
    }

    pub fn select_tool(&mut self, tool: EditorTool) -> bool {
        if self.is_drawing || self.tool == tool {
            return false;
        }
        if let Some(tool) = self.tool.paint_tool() {
            self.previous_paint_tool = tool;
        }
        self.tool = tool;
        true
    }

    pub fn cycle_tool(&mut self) -> bool {
        let tool = match self.tool {
            EditorTool::Paint(PaintTool::Brush) => PaintTool::Eraser.into(),
            EditorTool::Paint(PaintTool::Eraser) => PaintTool::Smudge.into(),
            EditorTool::Paint(PaintTool::Smudge) | EditorTool::Transform => PaintTool::Brush.into(),
        };
        self.select_tool(tool)
    }

    pub fn command_for_platform_shortcut(&self, event: &WindowEvent) -> Option<AppCommand> {
        if cfg!(target_os = "macos") {
            return None;
        }
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return None;
        };
        if event.state != ElementState::Pressed || event.repeat {
            return None;
        }
        let key = logical_shortcut_key(&event.logical_key)?;
        application_command_for_key(key, self.modifiers)
            .or_else(|| canvas_command_for_key(key, self.modifiers))
            .or_else(|| document_command_for_key(key, self.modifiers))
            .or_else(|| history_command_for_key(key, self.modifiers))
    }

    pub fn handle_event(
        &mut self,
        event: &WindowEvent,
        paint: &mut PaintRenderer,
        brush: &mut BrushSettings,
        brush_size_range: std::ops::RangeInclusive<f32>,
        pressure_state: &PressureStateHandle,
    ) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let next = [position.x as f32, position.y as f32];

                if let Some(mut drag) = self.rotation_drag {
                    let current_angle = pointer_angle(drag.anchor, next);
                    let raw_rotation =
                        drag.start_rotation + angle_delta(current_angle, drag.start_pointer_angle);
                    let (rotation, snapped_to) = snapped_rotation(raw_rotation, drag.snapped_to);
                    drag.snapped_to = snapped_to;
                    self.rotation_drag = Some(drag);
                    return paint.set_canvas_rotation(rotation);
                }
                if let Some(drag) = &self.eyedropper_drag {
                    let now = Instant::now();
                    let last_sample = drag.last_sample.map(|sample| sample.0);
                    return color_sample_is_due(last_sample, now)
                        && self.sample_color_at(paint, brush, next, now);
                }
                if let Some(mut drag) = self.resize_drag {
                    let Some(adjustment) = drag.resolve_adjustment_axis(next) else {
                        return false;
                    };
                    self.resize_drag = Some(drag);
                    return match adjustment {
                        BrushAdjustment::Size => {
                            let next_size = resized_brush_size(
                                drag.start_size,
                                drag.start_y,
                                next[1],
                                brush_size_range,
                            );
                            let changed = (brush.size - next_size).abs() > f32::EPSILON;
                            brush.size = next_size;
                            changed
                        }
                        BrushAdjustment::Opacity => {
                            let next_opacity =
                                adjusted_brush_opacity(drag.start_opacity, drag.start_x, next[0]);
                            let changed = (brush.opacity - next_opacity).abs() > f32::EPSILON;
                            brush.opacity = next_opacity;
                            changed
                        }
                    };
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
                    let time = self.sample_time(pressure_state);
                    let raw_point =
                        self.stroke_point_from_window(paint, next, *brush, pressure_state);
                    self.last_raw_point = Some(raw_point);
                    // Keep filter tuning independent of display scale and canvas zoom.
                    let scale_factor = self.scale_factor.unwrap_or(1.0);
                    let logical = scale_point(next, 1.0 / scale_factor);
                    let filtered = self.position_filter.filter(logical, time);
                    let document = paint.window_to_document(scale_point(filtered, scale_factor));
                    let point = StrokePoint {
                        x: document[0],
                        y: document[1],
                        ..raw_point
                    };
                    let smoothed_points = self.smoother.push(point);
                    let queued = self.queue_smoothed_points(paint, smoothed_points, *brush);
                    return queued > 0;
                }

                true
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if self.rotation_drag.is_some() => {
                self.rotation_drag = None;
                true
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if self.eyedropper_drag.is_some() => {
                self.finish_color_sampling(paint, brush);
                true
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if self.resize_drag.is_some() => {
                self.resize_drag = None;
                true
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if self.is_rotation_key_down => {
                    let anchor = paint.canvas_center_in_window();
                    self.rotation_drag = Some(RotationDrag {
                        anchor,
                        start_rotation: paint.canvas_rotation(),
                        start_pointer_angle: pointer_angle(anchor, self.cursor_pos),
                        snapped_to: None,
                    });
                    true
                }
                (ElementState::Pressed, MouseButton::Left) if self.is_eyedropper_active() => {
                    let started = self.eyedropper_drag.is_none();
                    self.eyedropper_drag.get_or_insert_default();
                    self.sample_color_at(paint, brush, self.cursor_pos, Instant::now()) || started
                }
                (ElementState::Pressed, MouseButton::Left)
                    if self.tool.paint_tool().is_some()
                        && resize_modifier_is_active(self.modifiers) =>
                {
                    self.begin_brush_resize_drag(brush.size, brush.opacity);
                    true
                }
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    true
                }
                (ElementState::Pressed, MouseButton::Left) => {
                    let Some(tool) = self.tool.paint_tool() else {
                        return false;
                    };
                    if !paint.can_paint() {
                        return false;
                    }
                    self.stroke_uses_pen_pressure = pressure_state.pen_input_active();
                    let time = self.sample_time(pressure_state);
                    let point = self.stroke_point_from_window(
                        paint,
                        self.cursor_pos,
                        *brush,
                        pressure_state,
                    );
                    if !paint.begin_stroke(tool, point, brush.rgba(), brush.opacity) {
                        return false;
                    }
                    self.is_drawing = true;
                    self.last_point = Some(point);
                    self.last_raw_point = Some(point);
                    let scale_factor = self.scale_factor.unwrap_or(1.0);
                    self.position_filter
                        .reset(scale_point(self.cursor_pos, 1.0 / scale_factor), time);
                    self.smoother.begin(point);
                    tool != PaintTool::Smudge && paint.queue_stamp(point)
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
            WindowEvent::ModifiersChanged(_)
                if self.eyedropper_drag.is_some() && !self.is_eyedropper_active() =>
            {
                self.finish_color_sampling(paint, brush);
                true
            }
            WindowEvent::ModifiersChanged(_)
                if self.resize_origin.is_some() && !resize_modifier_is_active(self.modifiers) =>
            {
                self.resize_origin = None;
                self.resize_drag = None;
                true
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyR) {
                    let pressed = event.state == ElementState::Pressed;
                    let active = pressed && self.modifiers.is_empty();
                    let changed = self.is_rotation_key_down != active;
                    self.is_rotation_key_down = active;
                    if !active {
                        self.rotation_drag = None;
                    }
                    if changed {
                        self.end_stroke(paint, *brush);
                        return true;
                    }
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
            WindowEvent::CursorLeft { .. } | WindowEvent::Focused(false) => {
                self.resize_origin = None;
                self.resize_drag = None;
                self.is_rotation_key_down = false;
                let was_rotating = self.rotation_drag.take().is_some();
                let was_sampling = self.eyedropper_drag.take().is_some();
                self.end_stroke(paint, *brush) || was_sampling || was_rotating
            }
            _ => false,
        }
    }

    pub fn finish_document_interaction(
        &mut self,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
    ) -> bool {
        self.resize_origin = None;
        self.resize_drag = None;
        self.is_rotation_key_down = false;
        self.rotation_drag = None;
        self.eyedropper_drag = None;
        let ended = self.end_stroke(paint, brush);
        paint.commit_layer_transform() || ended
    }

    fn sample_color_at(
        &mut self,
        paint: &PaintRenderer,
        brush: &mut BrushSettings,
        window_point: [f32; 2],
        sampled_at: Instant,
    ) -> bool {
        self.eyedropper_drag
            .as_mut()
            .expect("sampling requires an eyedropper drag")
            .last_sample = Some((sampled_at, window_point));
        let Some([red, green, blue]) = paint.sample_composited_color(window_point) else {
            return false;
        };
        let color = egui::Color32::from_rgb(red, green, blue);
        let changed = brush.color != color;
        brush.color = color;
        changed
    }

    fn finish_color_sampling(&mut self, paint: &PaintRenderer, brush: &mut BrushSettings) -> bool {
        let last_point = self
            .eyedropper_drag
            .as_ref()
            .and_then(|drag| drag.last_sample.map(|sample| sample.1));
        let changed = last_point != Some(self.cursor_pos)
            && self.sample_color_at(paint, brush, self.cursor_pos, Instant::now());
        self.eyedropper_drag = None;
        changed
    }

    fn begin_brush_resize_drag(&mut self, brush_size: f32, brush_opacity: f32) {
        self.resize_origin.get_or_insert(self.cursor_pos);
        self.resize_drag = Some(BrushResizeDrag {
            start_x: self.cursor_pos[0],
            start_y: self.cursor_pos[1],
            start_size: brush_size,
            start_opacity: brush_opacity,
            adjustment: None,
        });
    }

    fn select_tool_for_key(&mut self, key: KeyCode) -> bool {
        if self.is_drawing {
            return false;
        }
        let Some(mut tool) = editor_tool_for_key(key, self.modifiers) else {
            return false;
        };
        if tool == EditorTool::Transform && self.tool == EditorTool::Transform {
            tool = self.previous_paint_tool();
        }
        self.select_tool(tool)
    }

    fn sample_time(&mut self, pressure_state: &PressureStateHandle) -> Duration {
        pressure_state
            .brush_sample()
            .1
            .unwrap_or_else(|| self.input_clock.get_or_insert_with(Instant::now).elapsed())
    }

    fn stroke_point_from_window(
        &self,
        paint: &PaintRenderer,
        window_point: [f32; 2],
        brush: BrushSettings,
        pressure_state: &PressureStateHandle,
    ) -> StrokePoint {
        let doc = paint.window_to_document(window_point);
        brush.stroke_point(
            doc,
            pressure_state.stroke_pressure(self.stroke_uses_pen_pressure),
        )
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
            let smoothed_points = if let Some(point) = self.last_raw_point {
                self.smoother.finish_at(point)
            } else {
                self.smoother.finish()
            };
            let queued = self.queue_smoothed_points(paint, smoothed_points, brush);
            paint.end_stroke();
            queued
        } else {
            self.smoother.reset();
            0
        };
        self.is_drawing = false;
        self.stroke_uses_pen_pressure = false;
        self.is_panning = false;
        self.last_point = None;
        self.last_raw_point = None;
        queued > 0 || was_active
    }
}

fn scale_point(point: [f32; 2], scale: f32) -> [f32; 2] {
    [point[0] * scale, point[1] * scale]
}

fn pointer_angle(center: [f32; 2], point: [f32; 2]) -> f32 {
    (point[1] - center[1]).atan2(point[0] - center[0])
}

fn angle_delta(angle: f32, origin: f32) -> f32 {
    (angle - origin + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn snapped_rotation(raw: f32, current_snap: Option<f32>) -> (f32, Option<f32>) {
    if let Some(snap) = current_snap
        && angle_delta(raw, snap).abs() <= ROTATION_SNAP_EXIT
    {
        return (snap, Some(snap));
    }
    let nearest = (raw / ROTATION_SNAP_INTERVAL).round() * ROTATION_SNAP_INTERVAL;
    if angle_delta(raw, nearest).abs() <= ROTATION_SNAP_ENTER {
        (nearest, Some(nearest))
    } else {
        (raw, None)
    }
}

fn keyboard_shortcut_for_key(
    key: KeyCode,
    state: ElementState,
    repeat: bool,
    modifiers: ModifiersState,
) -> Option<KeyboardShortcut> {
    if state != ElementState::Pressed || repeat {
        return None;
    }
    match (key, modifiers) {
        (KeyCode::Tab, modifiers) if modifiers.is_empty() => Some(KeyboardShortcut::ToggleSidebar),
        (KeyCode::Tab, ModifiersState::SHIFT) => Some(KeyboardShortcut::CycleTool),
        (KeyCode::KeyF, modifiers) if modifiers.is_empty() => {
            Some(KeyboardShortcut::FlipCanvasHorizontal)
        }
        (KeyCode::KeyF, ModifiersState::SHIFT) => Some(KeyboardShortcut::FlipCanvasVertical),
        _ => None,
    }
}

fn eyedropper_modifier_is_active(modifiers: ModifiersState) -> bool {
    modifiers == ModifiersState::ALT
}

fn color_sample_is_due(last_sample: Option<Instant>, now: Instant) -> bool {
    last_sample.is_none_or(|last_sample| {
        now.saturating_duration_since(last_sample) >= EYEDROPPER_DRAG_SAMPLE_INTERVAL
    })
}

fn resize_modifier_is_active(modifiers: ModifiersState) -> bool {
    modifiers.shift_key()
        && !modifiers.control_key()
        && !modifiers.alt_key()
        && !modifiers.super_key()
}

fn resized_brush_size(
    start_size: f32,
    start_y: f32,
    current_y: f32,
    range: std::ops::RangeInclusive<f32>,
) -> f32 {
    (start_size + start_y - current_y).clamp(*range.start(), *range.end())
}

fn adjusted_brush_opacity(start_opacity: f32, start_x: f32, current_x: f32) -> f32 {
    (start_opacity + (current_x - start_x) / 100.0).clamp(0.01, 1.0)
}

fn editor_tool_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<EditorTool> {
    if modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match key {
        KeyCode::KeyD | KeyCode::KeyB => Some(PaintTool::Brush.into()),
        KeyCode::KeyE => Some(PaintTool::Eraser.into()),
        KeyCode::KeyS => Some(PaintTool::Smudge.into()),
        KeyCode::KeyT => Some(EditorTool::Transform),
        _ => None,
    }
}

fn logical_shortcut_key(key: &Key) -> Option<KeyCode> {
    match key {
        Key::Character(character) => match character.as_str().to_ascii_lowercase().as_str() {
            "n" => Some(KeyCode::KeyN),
            "s" => Some(KeyCode::KeyS),
            "e" => Some(KeyCode::KeyE),
            "g" => Some(KeyCode::KeyG),
            "z" => Some(KeyCode::KeyZ),
            "y" => Some(KeyCode::KeyY),
            "r" => Some(KeyCode::KeyR),
            "c" => Some(KeyCode::KeyC),
            "/" | "?" => Some(KeyCode::Slash),
            _ => None,
        },
        Key::Named(NamedKey::ArrowLeft) => Some(KeyCode::ArrowLeft),
        Key::Named(NamedKey::ArrowRight) => Some(KeyCode::ArrowRight),
        _ => None,
    }
}

fn application_command_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<AppCommand> {
    match (key, modifiers) {
        (KeyCode::KeyN, ModifiersState::CONTROL) => {
            Some(AppCommand::Navigation(NavigationCommand::NewArtwork))
        }
        (KeyCode::Slash, ModifiersState::SHIFT) => Some(AppCommand::Ui(UiCommand::ShowShortcuts)),
        _ => None,
    }
}

fn canvas_command_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<AppCommand> {
    if key == KeyCode::KeyR && modifiers == ModifiersState::SHIFT {
        return Some(AppCommand::Editor(EditorCommand::ResetCanvasRotation));
    }
    if !modifiers.control_key()
        || !modifiers.alt_key()
        || modifiers.shift_key()
        || modifiers.super_key()
    {
        return None;
    }
    match key {
        KeyCode::ArrowLeft => Some(AppCommand::Editor(EditorCommand::RotateCanvasLeft)),
        KeyCode::ArrowRight => Some(AppCommand::Editor(EditorCommand::RotateCanvasRight)),
        KeyCode::KeyC => Some(AppCommand::Editor(EditorCommand::RequestCanvasResize)),
        _ => None,
    }
}

fn document_command_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<AppCommand> {
    if !modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match (key, modifiers.shift_key()) {
        (KeyCode::KeyS, false) => Some(AppCommand::Editor(EditorCommand::SaveArtwork)),
        (KeyCode::KeyE, true) => Some(AppCommand::Editor(EditorCommand::ExportPng)),
        (KeyCode::KeyG, false) => Some(AppCommand::Navigation(NavigationCommand::ShowGallery)),
        _ => None,
    }
}

fn history_command_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<AppCommand> {
    if !modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match (key, modifiers.shift_key()) {
        (KeyCode::KeyZ, false) => Some(AppCommand::Editor(EditorCommand::Undo)),
        (KeyCode::KeyZ, true) | (KeyCode::KeyY, false) => {
            Some(AppCommand::Editor(EditorCommand::Redo))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
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
    fn horizontal_drag_adjusts_and_clamps_brush_opacity() {
        assert_eq!(adjusted_brush_opacity(0.5, 100.0, 120.0), 0.7);
        assert_eq!(adjusted_brush_opacity(0.5, 100.0, 80.0), 0.3);
        assert_eq!(adjusted_brush_opacity(0.5, 100.0, 200.0), 1.0);
        assert_eq!(adjusted_brush_opacity(0.5, 100.0, 0.0), 0.01);
    }

    #[test]
    fn first_drag_movement_locks_the_adjusted_setting() {
        let mut horizontal = BrushResizeDrag {
            start_x: 100.0,
            start_y: 200.0,
            start_size: 50.0,
            start_opacity: 0.5,
            adjustment: None,
        };
        assert_eq!(horizontal.resolve_adjustment_axis([100.0, 200.0]), None);
        assert_eq!(
            horizontal.resolve_adjustment_axis([120.0, 201.0]),
            Some(BrushAdjustment::Opacity)
        );
        assert_eq!(
            horizontal.resolve_adjustment_axis([101.0, 250.0]),
            Some(BrushAdjustment::Opacity)
        );

        let mut vertical = horizontal;
        vertical.adjustment = None;
        assert_eq!(
            vertical.resolve_adjustment_axis([101.0, 220.0]),
            Some(BrushAdjustment::Size)
        );
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
        input.begin_brush_resize_drag(48.0, 0.5);

        assert_eq!(input.brush_cursor_pos(), None);
        assert_eq!(input.brush_resize_pos(), Some([20.0, 30.0]));
        assert!(input.is_resizing_brush());
        assert!(input.has_active_document_drag());
        assert!(input.brush_resize_is_anchored());

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
        input.begin_brush_resize_drag(48.0, 0.5);
        input.cursor_pos = [40.0, 80.0];
        input.begin_brush_resize_drag(64.0, 0.75);

        assert_eq!(input.resize_origin, Some([20.0, 30.0]));
        let drag = input.resize_drag.unwrap();
        assert_eq!(drag.start_x, 40.0);
        assert_eq!(drag.start_y, 80.0);
        assert_eq!(drag.start_size, 64.0);
        assert_eq!(drag.start_opacity, 0.75);
        assert_eq!(drag.adjustment, None);
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
}
