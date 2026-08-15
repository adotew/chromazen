use std::sync::Arc;

use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys},
    window::{Window, WindowAttributes},
};

use crate::{
    config::LoadedBrushPreset,
    paint::{
        BrushSettings, BrushSpacing, PaintTool, PressureSettings, StrokePoint, StrokeSmoother,
    },
    renderer::PaintRenderer,
};

enum WebEvent {
    RendererReady(Result<PaintRenderer, String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrushKind {
    Charcoal,
    Sketch,
    Rounded,
    Rectangle,
}

impl BrushKind {
    const ALL: [Self; 4] = [Self::Charcoal, Self::Sketch, Self::Rounded, Self::Rectangle];

    fn label(self) -> &'static str {
        match self {
            Self::Charcoal => "Charcoal",
            Self::Sketch => "Sketch",
            Self::Rounded => "Rounded",
            Self::Rectangle => "Rectangle",
        }
    }

    fn load(self) -> LoadedBrushPreset {
        match self {
            Self::Charcoal => LoadedBrushPreset::bundled_charcoal(),
            Self::Sketch => LoadedBrushPreset::bundled_sketch(),
            Self::Rounded => LoadedBrushPreset::bundled_rounded(),
            Self::Rectangle => LoadedBrushPreset::bundled_rectangle(),
        }
    }
}

enum UiCommand {
    SetTool(PaintTool),
    SetBrush(BrushKind),
    Undo,
    Redo,
    Clear,
    Fit,
}

struct WebGui {
    context: egui::Context,
    state: EguiWinitState,
    renderer: EguiRenderer,
}

impl WebGui {
    fn new(window: &Window, paint: &PaintRenderer) -> Self {
        let context = egui::Context::default();
        let mut style = (*context.global_style()).clone();
        style.visuals = egui::Visuals::dark();
        context.set_global_style(style);
        let state = EguiWinitState::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(paint.device().limits().max_texture_dimension_2d as usize),
        );
        let renderer = EguiRenderer::new(
            paint.device(),
            paint.surface_format(),
            RendererOptions::default(),
        );
        Self {
            context,
            state,
            renderer,
        }
    }

    fn run(
        &mut self,
        window: &Window,
        tool: PaintTool,
        brush_kind: BrushKind,
        brush: &mut BrushSettings,
        can_undo: bool,
        can_redo: bool,
    ) -> (egui::FullOutput, Vec<UiCommand>) {
        let raw_input = self.state.take_egui_input(window);
        let mut commands = Vec::new();
        let output = self.context.run_ui(raw_input, |ui| {
            egui::Panel::top("browser toolbar")
                .frame(
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(28, 29, 33))
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show_inside(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Chromazen");
                        ui.separator();
                        for (candidate, label) in [
                            (PaintTool::Brush, "Brush  B"),
                            (PaintTool::Eraser, "Eraser  E"),
                            (PaintTool::Smudge, "Smudge  S"),
                        ] {
                            if ui.selectable_label(tool == candidate, label).clicked() {
                                commands.push(UiCommand::SetTool(candidate));
                            }
                        }
                        ui.separator();
                        egui::ComboBox::from_id_salt("browser brush preset")
                            .selected_text(brush_kind.label())
                            .show_ui(ui, |ui| {
                                for kind in BrushKind::ALL {
                                    if ui
                                        .selectable_label(brush_kind == kind, kind.label())
                                        .clicked()
                                    {
                                        commands.push(UiCommand::SetBrush(kind));
                                    }
                                }
                            });
                        ui.label("Size");
                        ui.add(egui::Slider::new(&mut brush.size, 1.0..=800.0).logarithmic(true));
                        ui.color_edit_button_srgba(&mut brush.color);
                        ui.separator();
                        if ui
                            .add_enabled(can_undo, egui::Button::new("Undo"))
                            .clicked()
                        {
                            commands.push(UiCommand::Undo);
                        }
                        if ui
                            .add_enabled(can_redo, egui::Button::new("Redo"))
                            .clicked()
                        {
                            commands.push(UiCommand::Redo);
                        }
                        if ui.button("Clear").clicked() {
                            commands.push(UiCommand::Clear);
                        }
                        if ui.button("Fit").clicked() {
                            commands.push(UiCommand::Fit);
                        }
                        ui.separator();
                        ui.weak("Wheel: zoom · right/middle drag: pan · no saving");
                    });
                });
        });
        (output, commands)
    }
}

#[derive(Default)]
struct WebInput {
    cursor: [f32; 2],
    cursor_inside: bool,
    drawing: bool,
    panning: bool,
    last_pan: [f32; 2],
    last_point: Option<StrokePoint>,
    smoother: StrokeSmoother,
    active_touch: Option<u64>,
}

impl WebInput {
    fn begin_stroke(
        &mut self,
        paint: &mut PaintRenderer,
        tool: PaintTool,
        brush: BrushSettings,
        position: [f32; 2],
        pressure: f32,
    ) -> bool {
        let point = brush.stroke_point(paint.window_to_document(position), pressure);
        if !paint.begin_stroke(tool, point, brush.rgba()) {
            return false;
        }
        self.drawing = true;
        self.last_point = Some(point);
        self.smoother.begin(point);
        tool == PaintTool::Smudge || paint.queue_stamp(point)
    }

    fn move_stroke(
        &mut self,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
        position: [f32; 2],
        pressure: f32,
    ) -> bool {
        let point = brush.stroke_point(paint.window_to_document(position), pressure);
        let points = self.smoother.push(point);
        self.queue_points(paint, brush, points) > 0
    }

    fn queue_points(
        &mut self,
        paint: &mut PaintRenderer,
        brush: BrushSettings,
        points: Vec<StrokePoint>,
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
        if !self.drawing {
            return false;
        }
        let points = self.smoother.finish();
        self.queue_points(paint, brush, points);
        paint.end_stroke();
        self.drawing = false;
        self.last_point = None;
        true
    }

    fn cancel_interaction(&mut self, paint: &mut PaintRenderer, brush: BrushSettings) -> bool {
        let changed = self.end_stroke(paint, brush) || self.panning;
        self.panning = false;
        self.active_touch = None;
        changed
    }
}

struct WebApp {
    proxy: EventLoopProxy<WebEvent>,
    window: Option<Arc<Window>>,
    paint: Option<PaintRenderer>,
    gui: Option<WebGui>,
    input: WebInput,
    brush: BrushSettings,
    brush_kind: BrushKind,
    tool: PaintTool,
    modifiers: ModifiersState,
    initializing: bool,
    pending_surface_size: Option<PhysicalSize<u32>>,
}

impl WebApp {
    fn new(proxy: EventLoopProxy<WebEvent>) -> Self {
        let brush_kind = BrushKind::Charcoal;
        Self {
            proxy,
            window: None,
            paint: None,
            gui: None,
            input: WebInput::default(),
            brush: settings_for_preset(&brush_kind.load()),
            brush_kind,
            tool: PaintTool::Brush,
            modifiers: ModifiersState::empty(),
            initializing: false,
            pending_surface_size: None,
        }
    }

    fn process_ui_commands(&mut self, commands: Vec<UiCommand>) {
        for command in commands {
            match command {
                UiCommand::SetTool(tool) => {
                    if let Some(paint) = self.paint.as_mut() {
                        self.input.end_stroke(paint, self.brush);
                    }
                    self.tool = tool;
                }
                UiCommand::SetBrush(kind) => {
                    let loaded = kind.load();
                    if let Some(paint) = self.paint.as_mut()
                        && paint.try_set_brush_preset(&loaded).unwrap_or(false)
                    {
                        let color = self.brush.color;
                        self.brush = settings_for_preset(&loaded);
                        self.brush.color = color;
                        self.brush_kind = kind;
                    }
                }
                UiCommand::Undo => {
                    if let Some(paint) = self.paint.as_mut() {
                        self.input.end_stroke(paint, self.brush);
                        paint.undo();
                    }
                }
                UiCommand::Redo => {
                    if let Some(paint) = self.paint.as_mut() {
                        self.input.end_stroke(paint, self.brush);
                        paint.redo();
                    }
                }
                UiCommand::Clear => {
                    if let Some(paint) = self.paint.as_mut() {
                        self.input.end_stroke(paint, self.brush);
                        paint.clear_canvas();
                    }
                }
                UiCommand::Fit => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.fit_to_screen();
                    }
                }
            }
        }
    }

    fn handle_keyboard(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.state != ElementState::Pressed || event.repeat {
            return false;
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return false;
        };
        let command = match code {
            KeyCode::KeyB if self.modifiers.is_empty() => {
                Some(UiCommand::SetTool(PaintTool::Brush))
            }
            KeyCode::KeyE if self.modifiers.is_empty() => {
                Some(UiCommand::SetTool(PaintTool::Eraser))
            }
            KeyCode::KeyS if self.modifiers.is_empty() => {
                Some(UiCommand::SetTool(PaintTool::Smudge))
            }
            KeyCode::KeyZ if self.modifiers.control_key() || self.modifiers.super_key() => {
                Some(if self.modifiers.shift_key() {
                    UiCommand::Redo
                } else {
                    UiCommand::Undo
                })
            }
            KeyCode::KeyY if self.modifiers.control_key() || self.modifiers.super_key() => {
                Some(UiCommand::Redo)
            }
            KeyCode::Digit0 => Some(UiCommand::Fit),
            _ => None,
        };
        if let Some(command) = command {
            self.process_ui_commands(vec![command]);
            true
        } else {
            false
        }
    }

    fn handle_paint_event(&mut self, event: &WindowEvent, consumed: bool) -> bool {
        let Some(paint) = self.paint.as_mut() else {
            return false;
        };
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let next = [position.x as f32, position.y as f32];
                self.input.cursor = next;
                self.input.cursor_inside = true;
                if self.input.panning {
                    let delta = [
                        next[0] - self.input.last_pan[0],
                        next[1] - self.input.last_pan[1],
                    ];
                    self.input.last_pan = next;
                    paint.pan_by_window_delta(delta);
                    true
                } else if self.input.drawing {
                    self.input.move_stroke(paint, self.brush, next, 1.0)
                } else {
                    true
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.input.cursor_inside = false;
                self.input.cancel_interaction(paint, self.brush)
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if !consumed => {
                    self.input
                        .begin_stroke(paint, self.tool, self.brush, self.input.cursor, 1.0)
                }
                (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) if !consumed => {
                    self.input.panning = true;
                    self.input.last_pan = self.input.cursor;
                    true
                }
                (ElementState::Released, MouseButton::Left) => {
                    self.input.end_stroke(paint, self.brush)
                }
                (ElementState::Released, MouseButton::Middle | MouseButton::Right) => {
                    std::mem::replace(&mut self.input.panning, false)
                }
                _ => false,
            },
            WindowEvent::MouseWheel { delta, .. } if !consumed => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(position) => -(position.y as f32) / 120.0,
                };
                if amount != 0.0 {
                    paint.apply_zoom_at(if amount > 0.0 { 1.1 } else { 0.9 }, self.input.cursor);
                    true
                } else {
                    false
                }
            }
            WindowEvent::Touch(touch) => self.handle_touch(*touch, consumed),
            WindowEvent::Focused(false) => self.input.cancel_interaction(paint, self.brush),
            _ => false,
        }
    }

    fn handle_touch(&mut self, touch: Touch, consumed: bool) -> bool {
        let Some(paint) = self.paint.as_mut() else {
            return false;
        };
        let position = [touch.location.x as f32, touch.location.y as f32];
        let pressure = touch
            .force
            .map_or(1.0, |force| force.normalized() as f32)
            .clamp(0.01, 1.0);
        match touch.phase {
            TouchPhase::Started if !consumed && self.input.active_touch.is_none() => {
                self.input.active_touch = Some(touch.id);
                self.input.cursor = position;
                self.input.cursor_inside = true;
                self.input
                    .begin_stroke(paint, self.tool, self.brush, position, pressure)
            }
            TouchPhase::Moved if self.input.active_touch == Some(touch.id) => {
                self.input.cursor = position;
                self.input
                    .move_stroke(paint, self.brush, position, pressure)
            }
            TouchPhase::Ended | TouchPhase::Cancelled
                if self.input.active_touch == Some(touch.id) =>
            {
                self.input.active_touch = None;
                self.input.end_stroke(paint, self.brush)
            }
            _ => false,
        }
    }

    fn render(&mut self, window: &Window) {
        let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_mut()) else {
            return;
        };
        let (full_output, commands) = gui.run(
            window,
            self.tool,
            self.brush_kind,
            &mut self.brush,
            paint.can_undo(),
            paint.can_redo(),
        );
        self.process_ui_commands(commands);

        let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_mut()) else {
            return;
        };
        gui.state
            .handle_platform_output(window, full_output.platform_output);
        for (id, image_delta) in &full_output.textures_delta.set {
            gui.renderer
                .update_texture(paint.device(), paint.queue(), *id, image_delta);
        }
        let paint_jobs = gui
            .context
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let frame = match paint.acquire_frame() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                paint.reconfigure_surface();
                return;
            }
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = paint
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("browser frame encoder"),
            });
        // Some browser WebGPU implementations invalidate the frame when the textured brush
        // outline is first drawn. The browser uses a CSS crosshair instead; painting still uses
        // the selected brush texture and size.
        paint.render_to_view(&mut encoder, &view, None);
        let screen = ScreenDescriptor {
            size_in_pixels: paint.surface_size(),
            pixels_per_point: full_output.pixels_per_point,
        };
        let user_buffers = gui.renderer.update_buffers(
            paint.device(),
            paint.queue(),
            &mut encoder,
            &paint_jobs,
            &screen,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("browser egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            gui.renderer.render(&mut pass, &paint_jobs, &screen);
        }
        paint.queue().submit(
            user_buffers
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();
        for id in &full_output.textures_delta.free {
            gui.renderer.free_texture(id);
        }
        if paint.has_pending_stamps() {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler<WebEvent> for WebApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.initializing {
            return;
        }
        let Some(canvas) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("chromazen-canvas"))
            .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        else {
            set_status("Could not find the #chromazen-canvas element.", true);
            return;
        };
        let attributes = WindowAttributes::default()
            .with_title("Chromazen")
            .with_canvas(Some(canvas))
            .with_prevent_default(true)
            .with_focusable(true);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                set_status(
                    &format!("Could not create the browser canvas: {error}"),
                    true,
                );
                return;
            }
        };
        self.window = Some(window.clone());
        self.initializing = true;
        let proxy = self.proxy.clone();
        let preset = self.brush_kind.load();
        wasm_bindgen_futures::spawn_local(async move {
            let renderer = PaintRenderer::new(window, &preset).await;
            let _ = proxy.send_event(WebEvent::RendererReady(renderer));
        });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: WebEvent) {
        self.initializing = false;
        match event {
            WebEvent::RendererReady(Ok(mut paint)) => {
                let Some(window) = self.window.as_ref() else {
                    return;
                };
                // The first ResizeObserver notification can arrive while WebGPU is still
                // initializing. Apply the remembered size so the surface does not stay at
                // winit's temporary 1x1 backing size.
                if let Some(size) = self.pending_surface_size
                    && size.width > 0
                    && size.height > 0
                {
                    paint.resize(size);
                    paint.fit_to_screen();
                }
                self.gui = Some(WebGui::new(window, &paint));
                self.paint = Some(paint);
                set_status("", false);
                window.request_redraw();
            }
            WebEvent::RendererReady(Err(error)) => {
                set_status(&format!("WebGPU initialization failed: {error}"), true);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        match &event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.pending_surface_size = Some(*size);
                if let Some(paint) = self.paint.as_mut() {
                    paint.resize(*size);
                }
                window.request_redraw();
                return;
            }
            WindowEvent::RedrawRequested => {
                self.render(&window);
                return;
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if self.handle_keyboard(event) => {
                window.request_redraw();
                return;
            }
            _ => {}
        }
        let response = self
            .gui
            .as_mut()
            .map(|gui| gui.state.on_window_event(&window, &event));
        let changed = self.handle_paint_event(
            &event,
            response.as_ref().is_some_and(|response| response.consumed),
        );
        if changed || response.is_some_and(|response| response.repaint) {
            window.request_redraw();
        }
    }
}

fn settings_for_preset(loaded: &LoadedBrushPreset) -> BrushSettings {
    BrushSettings {
        color: egui::Color32::from_rgb(170, 187, 204),
        size: loaded.preset.size.default,
        pressure: PressureSettings {
            min_size: loaded.preset.pressure.min_size,
            min_opacity: loaded.preset.pressure.min_opacity,
            full_opacity_pressure: loaded.preset.pressure.full_opacity_pressure,
            opacity_gamma: loaded.preset.pressure.opacity_gamma,
        },
        spacing: BrushSpacing {
            ratio: loaded.preset.spacing.ratio,
            minimum: loaded.preset.spacing.minimum,
        },
    }
}

fn set_status(message: &str, error: bool) {
    let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("loading"))
    else {
        return;
    };
    element.set_text_content((!message.is_empty()).then_some(message));
    let class_name = if message.is_empty() {
        "hidden"
    } else if error {
        "error"
    } else {
        ""
    };
    element.set_class_name(class_name);
}

pub(crate) fn report_gpu_error(error: wgpu::Error) {
    let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("loading"))
    else {
        return;
    };
    let previous = element.text_content().unwrap_or_default();
    let separator = if previous.is_empty() { "" } else { "\n\n" };
    element.set_text_content(Some(&format!(
        "{previous}{separator}WebGPU rendering failed: {error}"
    )));
    element.set_class_name("error");
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(|info| {
        set_status(&format!("Chromazen stopped: {info}"), true);
        console_error_panic_hook::hook(info);
    }));
    set_status("Starting WebGPU…", false);
    let event_loop = EventLoop::<WebEvent>::with_user_event()
        .build()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let proxy = event_loop.create_proxy();
    event_loop.spawn_app(WebApp::new(proxy));
    Ok(())
}
