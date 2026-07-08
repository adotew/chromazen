use std::{sync::Arc, time::Instant};

use egui::{Color32, ViewportId};
use egui_wgpu::{Renderer as EguiRenderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

use crate::renderer::{PaintRenderer, StrokePoint};

const WINDOW_TITLE: &str = "minipaint-rs";
const DEFAULT_BRUSH_SIZE: f32 = 300.0;
const MIN_BRUSH_SIZE: f32 = 1.0;
const MAX_BRUSH_SIZE: f32 = 2000.0;

struct GuiState {
    context: egui::Context,
    state: EguiWinitState,
    renderer: EguiRenderer,
    brush_color: Color32,
    brush_size: f32,
}

pub struct App {
    window: Option<Arc<Window>>,
    paint: Option<PaintRenderer>,
    gui: Option<GuiState>,
    cursor_pos: [f32; 2],
    is_drawing: bool,
    is_panning: bool,
    is_space_down: bool,
    last_point: Option<StrokePoint>,
    last_pan_pos: [f32; 2],
    last_frame: Instant,
    frame_ms: f32,
    fps: f32,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            paint: None,
            gui: None,
            cursor_pos: [0.0, 0.0],
            is_drawing: false,
            is_panning: false,
            is_space_down: false,
            last_point: None,
            last_pan_pos: [0.0, 0.0],
            last_frame: Instant::now(),
            frame_ms: 0.0,
            fps: 0.0,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_title(WINDOW_TITLE)
                        .with_resizable(true)
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 900.0)),
                )
                .expect("failed to create window"),
        );

        let paint = pollster::block_on(PaintRenderer::new(window.clone()))
            .expect("failed to initialize wgpu paint renderer");
        let egui_context = egui::Context::default();
        let egui_state = EguiWinitState::new(
            egui_context.clone(),
            ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(paint.device().limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer = EguiRenderer::new(
            paint.device(),
            paint.surface_format(),
            RendererOptions::default(),
        );

        self.window = Some(window.clone());
        self.paint = Some(paint);
        self.gui = Some(GuiState {
            context: egui_context,
            state: egui_state,
            renderer: egui_renderer,
            brush_color: Color32::from_rgb(170, 187, 204),
            brush_size: DEFAULT_BRUSH_SIZE,
        });
        self.last_frame = Instant::now();
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref().cloned() else { return; };
        if window.id() != window_id {
            return;
        }

        let Some(gui) = self.gui.as_mut() else { return; };
        let egui_response = gui.state.on_window_event(window.as_ref(), &event);
        if egui_response.repaint {
            window.request_redraw();
        }

        if !egui_response.consumed {
            self.handle_paint_input(&event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.resize(size);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(paint) = self.paint.as_mut() {
                    paint.resize(window.inner_size());
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.render(window.as_ref());
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl App {
    fn handle_paint_input(&mut self, event: &WindowEvent) {
        let Some(paint) = self.paint.as_mut() else { return; };
        let Some(gui) = self.gui.as_ref() else { return; };

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let next = [position.x as f32, position.y as f32];
                self.cursor_pos = next;

                if self.is_panning {
                    paint.pan_by_window_delta([next[0] - self.last_pan_pos[0], next[1] - self.last_pan_pos[1]]);
                    self.last_pan_pos = next;
                    return;
                }

                if self.is_drawing {
                    let doc = paint.window_to_document(next);
                    let point = StrokePoint {
                        x: doc[0],
                        y: doc[1],
                        radius: gui.brush_size * 0.5,
                        opacity: 1.0,
                    };
                    if let Some(previous) = self.last_point {
                        paint.stamp_line(previous, point, color32_to_rgba(gui.brush_color));
                    }
                    self.last_point = Some(point);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match (state, button) {
                (ElementState::Pressed, MouseButton::Left) if self.is_space_down => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                }
                (ElementState::Pressed, MouseButton::Left) => {
                    let doc = paint.window_to_document(self.cursor_pos);
                    let point = StrokePoint {
                        x: doc[0],
                        y: doc[1],
                        radius: gui.brush_size * 0.5,
                        opacity: 1.0,
                    };
                    self.is_drawing = true;
                    self.last_point = Some(point);
                    paint.begin_stroke();
                    paint.queue_stamp(point, color32_to_rgba(gui.brush_color));
                }
                (ElementState::Pressed, MouseButton::Middle | MouseButton::Right) => {
                    self.is_panning = true;
                    self.last_pan_pos = self.cursor_pos;
                }
                (ElementState::Released, _) => {
                    if self.is_drawing {
                        paint.end_stroke();
                    }
                    self.is_drawing = false;
                    self.is_panning = false;
                    self.last_point = None;
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(pos) => -(pos.y as f32) / 120.0,
                };
                if scroll != 0.0 {
                    let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
                    paint.apply_zoom_at(factor, self.cursor_pos);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.physical_key == PhysicalKey::Code(KeyCode::Space) {
                    self.is_space_down = event.state == ElementState::Pressed;
                    if !self.is_space_down {
                        self.is_panning = false;
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.is_drawing {
                    paint.end_stroke();
                }
                self.is_drawing = false;
                self.is_panning = false;
                self.last_point = None;
            }
            _ => {}
        }
    }

    fn render(&mut self, window: &Window) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        self.frame_ms = dt * 1000.0;
        self.fps = if dt > 0.0 { 1.0 / dt } else { 0.0 };

        let Some(paint) = self.paint.as_mut() else { return; };
        let Some(gui) = self.gui.as_mut() else { return; };
        if paint.surface_size()[0] == 0 || paint.surface_size()[1] == 0 {
            return;
        }

        let raw_input = gui.state.take_egui_input(window);
        let stats_before = paint.stats();
        let document_size = paint.document_size();
        let zoom = paint.zoom();
        let offset = paint.offset();
        let frame_ms = self.frame_ms;
        let fps = self.fps;
        let full_output = gui.context.run_ui(raw_input, |ui| {
            egui::Window::new("minipaint-rs")
                .default_pos([12.0, 12.0])
                .default_width(260.0)
                .show(ui.ctx(), |ui| {
                    ui.label("Minimal wgpu brush performance port");
                    ui.separator();
                    ui.add(
                        egui::Slider::new(&mut gui.brush_size, MIN_BRUSH_SIZE..=MAX_BRUSH_SIZE)
                            .text("Brush size")
                            .suffix(" px"),
                    );
                    egui::color_picker::color_picker_color32(
                        ui,
                        &mut gui.brush_color,
                        egui::color_picker::Alpha::Opaque,
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            paint.clear_canvas();
                        }
                        if ui.button("Fit").clicked() {
                            paint.fit_to_screen();
                        }
                        if ui.button("100%").clicked() {
                            paint.zoom_to_100();
                        }
                    });
                    ui.separator();
                    ui.label(format!("Canvas: {} × {}", document_size[0], document_size[1]));
                    ui.label(format!("Zoom: {:.1}%", zoom * 100.0));
                    ui.label(format!("Offset: {:.0}, {:.0}", offset[0], offset[1]));
                    ui.label(format!("Frame: {:.2} ms ({:.0} FPS)", frame_ms, fps));
                    ui.label(format!("Stamps/frame: {}", stats_before.stamps_last_frame));
                    ui.label(format!("Pending stamps: {}", stats_before.pending_stamps));
                    ui.label(format!("Total stamps: {}", stats_before.total_stamps));
                    ui.separator();
                    ui.small("Paint: left drag · Pan: middle/right drag or Space+left · Zoom: wheel");
                });
        });
        gui.state.handle_platform_output(window, full_output.platform_output);

        for (id, image_delta) in &full_output.textures_delta.set {
            gui.renderer.update_texture(paint.device(), paint.queue(), *id, image_delta);
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
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = paint.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame encoder"),
        });

        paint.render_to_view(&mut encoder, &view);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: paint.surface_size(),
            pixels_per_point: full_output.pixels_per_point,
        };
        let user_cmd_bufs = gui.renderer.update_buffers(
            paint.device(),
            paint.queue(),
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            gui.renderer.render(&mut pass, &paint_jobs, &screen_descriptor);
        }

        paint.queue().submit(user_cmd_bufs.into_iter().chain(std::iter::once(encoder.finish())));
        frame.present();

        for id in &full_output.textures_delta.free {
            gui.renderer.free_texture(id);
        }
    }
}

fn color32_to_rgba(color: Color32) -> [f32; 4] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        1.0,
    ]
}

pub fn run() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("event loop error");
}
