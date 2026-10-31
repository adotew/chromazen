use std::time::{Duration, Instant};

use chromazen_canvas::Canvas;
use winit::{
    dpi::PhysicalPosition,
    event::{DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey},
};

use crate::{
    paint::{BrushSettings, PaintTool, StrokePoint, StrokePositionFilter, StrokeSmoother},
    platform::{PenEvent, PressureStateHandle},
};

use super::command::{AppCommand, EditorCommand, NavigationCommand, UiCommand};

const EYEDROPPER_DRAG_SAMPLE_INTERVAL: Duration = Duration::from_millis(33);
const ROTATION_SNAP_INTERVAL: f32 = std::f32::consts::FRAC_PI_2;
const ROTATION_SNAP_ENTER: f32 = 5.0_f32.to_radians();
const ROTATION_SNAP_EXIT: f32 = 8.0_f32.to_radians();
const LASSO_MIN_POINT_SPACING: f32 = 2.0;

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

#[derive(Debug, Clone, Copy)]
struct BrushResizeDrag {
    start_y: f32,
    start_size: f32,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum QuickMenuRequest {
    /// Window position in physical pixels.
    Open {
        position: [f32; 2],
        held_by_key: bool,
    },
    ReleaseKey,
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
    Select,
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
            Self::Transform | Self::Select => None,
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
    lasso: Option<Vec<[f32; 2]>>,
    quick_menu_request: Option<QuickMenuRequest>,
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

    pub(crate) fn lasso_points(&self) -> Option<&[[f32; 2]]> {
        self.lasso.as_deref()
    }

    pub(crate) fn take_quick_menu_request(&mut self) -> Option<QuickMenuRequest> {
        self.quick_menu_request.take()
    }

    pub fn has_active_document_drag(&self) -> bool {
        self.is_drawing
            || self.lasso.is_some()
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
        if self.is_drawing || self.lasso.is_some() || self.tool == tool {
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
            EditorTool::Paint(PaintTool::Smudge) | EditorTool::Transform | EditorTool::Select => {
                PaintTool::Brush.into()
            }
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
        paint: &mut Canvas,
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
                if let Some(points) = &mut self.lasso {
                    let point = paint.window_to_document(next);
                    let min_distance = LASSO_MIN_POINT_SPACING / paint.zoom();
                    let far_enough = points.last().is_none_or(|last| {
                        (point[0] - last[0]).hypot(point[1] - last[1]) >= min_distance
                    });
                    if far_enough {
                        points.push(point);
                    }
                    return far_enough;
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
                    let preview_points = self.smoother.preview_through(raw_point);
                    let preview_changed = self.last_point.is_some_and(|from| {
                        Canvas::update_stroke_preview(paint, from, &preview_points, brush.spacing)
                    });
                    return queued > 0 || preview_changed;
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
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if self.lasso.is_some() => self.finish_lasso(paint),
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
                    self.begin_brush_resize_drag(brush.size);
                    true
                }
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                    true
                }
                (ElementState::Pressed, MouseButton::Left) if self.tool == EditorTool::Select => {
                    self.lasso = Some(vec![paint.window_to_document(self.cursor_pos)]);
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
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyQ) {
                    self.quick_menu_request = quick_menu_key_request(
                        event.state,
                        event.repeat,
                        self.modifiers,
                        self.has_active_document_drag(),
                        self.cursor_pos,
                    );
                    return self.quick_menu_request.is_some();
                }
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
                let was_selecting = self.finish_lasso(paint);
                self.end_stroke(paint, *brush) || was_sampling || was_rotating || was_selecting
            }
            _ => false,
        }
    }

    pub fn finish_document_interaction(
        &mut self,
        paint: &mut Canvas,
        brush: BrushSettings,
    ) -> bool {
        self.resize_origin = None;
        self.resize_drag = None;
        self.is_rotation_key_down = false;
        self.rotation_drag = None;
        self.eyedropper_drag = None;
        let selected = self.finish_lasso(paint);
        let ended = self.end_stroke(paint, brush);
        paint.commit_layer_transform() || ended || selected
    }

    fn finish_lasso(&mut self, paint: &mut Canvas) -> bool {
        let Some(points) = self.lasso.take() else {
            return false;
        };
        if !paint.set_selection(points) {
            paint.clear_selection();
        }
        true
    }

    fn sample_color_at(
        &mut self,
        paint: &Canvas,
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

    fn finish_color_sampling(&mut self, paint: &Canvas, brush: &mut BrushSettings) -> bool {
        let last_point = self
            .eyedropper_drag
            .as_ref()
            .and_then(|drag| drag.last_sample.map(|sample| sample.1));
        let changed = last_point != Some(self.cursor_pos)
            && self.sample_color_at(paint, brush, self.cursor_pos, Instant::now());
        self.eyedropper_drag = None;
        changed
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
        paint: &Canvas,
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
        paint: &mut Canvas,
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

    fn end_stroke(&mut self, paint: &mut Canvas, brush: BrushSettings) -> bool {
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

fn quick_menu_key_request(
    state: ElementState,
    repeat: bool,
    modifiers: ModifiersState,
    document_busy: bool,
    cursor_pos: [f32; 2],
) -> Option<QuickMenuRequest> {
    match state {
        ElementState::Pressed if !repeat && modifiers.is_empty() && !document_busy => {
            Some(QuickMenuRequest::Open {
                position: cursor_pos,
                held_by_key: true,
            })
        }
        ElementState::Pressed => None,
        ElementState::Released => Some(QuickMenuRequest::ReleaseKey),
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

fn editor_tool_for_key(key: KeyCode, modifiers: ModifiersState) -> Option<EditorTool> {
    if modifiers.control_key() || modifiers.alt_key() || modifiers.super_key() {
        return None;
    }
    match key {
        KeyCode::KeyD | KeyCode::KeyB => Some(PaintTool::Brush.into()),
        KeyCode::KeyE => Some(PaintTool::Eraser.into()),
        KeyCode::KeyS => Some(PaintTool::Smudge.into()),
        KeyCode::KeyT => Some(EditorTool::Transform),
        KeyCode::KeyL => Some(EditorTool::Select),
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
            "," => Some(KeyCode::Comma),
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
        (KeyCode::Comma, ModifiersState::CONTROL) => Some(AppCommand::Ui(UiCommand::ShowSettings)),
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
mod tests;
