mod input;
mod settings;
mod ui;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use egui_wgpu::ScreenDescriptor;
use winit::{
    application::ApplicationHandler,
    event::{StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes},
};

use self::{
    input::PaintInputController,
    settings::{SettingsCommand, SettingsController, SettingsEffect},
    ui::GuiLayer,
};
use crate::{
    platform::{MacosPressureMonitor, PressureStateHandle},
    renderer::PaintRenderer,
};

const WINDOW_TITLE: &str = "minipaint-rs";

struct RenderOutcome {
    repaint_delay: Duration,
    canvas_needs_redraw: bool,
}

pub struct App {
    window: Option<Arc<Window>>,
    paint: Option<PaintRenderer>,
    gui: Option<GuiLayer>,
    input: PaintInputController,
    pressure_state: PressureStateHandle,
    _pressure_monitor: Option<MacosPressureMonitor>,
    next_repaint: Option<Instant>,
    settings: SettingsController,
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

        let pressure_state = PressureStateHandle::default();
        let pressure_monitor =
            MacosPressureMonitor::install(window.clone(), pressure_state.clone())
                .expect("failed to initialize pressure monitor");
        let catalog = self.settings.take_startup_catalog();
        let startup_error = self.settings.take_startup_error();
        let paint = pollster::block_on(PaintRenderer::new(
            window.clone(),
            self.settings.active_brush(),
        ))
        .expect("failed to initialize wgpu paint renderer");
        let gui = GuiLayer::new(
            window.as_ref(),
            &paint,
            self.settings.config(),
            self.settings.active_brush(),
            catalog,
            startup_error,
        );

        self.window = Some(window.clone());
        self.paint = Some(paint);
        self.gui = Some(gui);
        self.pressure_state = pressure_state;
        self._pressure_monitor = pressure_monitor;
        window.request_redraw();
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

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.render(window.as_ref()),
            event => {
                let Some(gui) = self.gui.as_mut() else {
                    return;
                };
                let egui_response = gui.state.on_window_event(window.as_ref(), &event);
                let mut needs_redraw = egui_response.repaint;
                let egui_consumed = egui_response.consumed;

                if !egui_consumed
                    && let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref())
                {
                    needs_redraw |= self.input.handle_event(
                        &event,
                        paint,
                        gui.brush,
                        gui.stroke_smoothing,
                        &self.pressure_state,
                    );
                }

                match event {
                    WindowEvent::Resized(size) => {
                        if let Some(paint) = self.paint.as_mut() {
                            paint.resize(size);
                        }
                        needs_redraw = true;
                    }
                    WindowEvent::ScaleFactorChanged { .. } => {
                        if let Some(paint) = self.paint.as_mut() {
                            paint.resize(window.inner_size());
                        }
                        needs_redraw = true;
                    }
                    _ => {}
                }

                if needs_redraw {
                    self.next_repaint = None;
                    window.request_redraw();
                }
            }
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) && self.next_repaint.is_some() {
            self.request_scheduled_redraw(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.update_control_flow(event_loop);
    }
}

impl App {
    fn new(settings: SettingsController) -> Self {
        Self {
            window: None,
            paint: None,
            gui: None,
            input: PaintInputController::default(),
            pressure_state: PressureStateHandle::default(),
            _pressure_monitor: None,
            next_repaint: None,
            settings,
        }
    }

    fn render(&mut self, window: &Window) {
        let Some(paint) = self.paint.as_ref() else {
            return;
        };
        if paint.surface_size()[0] == 0 || paint.surface_size()[1] == 0 {
            return;
        }

        let (full_output, commands) = {
            let Some(gui) = self.gui.as_mut() else {
                return;
            };
            let output = gui.run(window);
            (output, gui.take_commands())
        };
        let settings_action_processed = !commands.is_empty();
        self.process_settings_commands(commands);

        let Some(outcome) = self.render_frame(window, full_output) else {
            return;
        };
        let brush_switched = self.apply_pending_brush_change();
        self.update_repaint_schedule(
            outcome.repaint_delay,
            window,
            outcome.canvas_needs_redraw || settings_action_processed || brush_switched,
        );
    }

    fn process_settings_commands(&mut self, commands: Vec<SettingsCommand>) {
        for command in commands {
            let Some(effect) = self.settings.handle_command(command) else {
                continue;
            };
            let Some(gui) = self.gui.as_mut() else {
                continue;
            };
            match effect {
                SettingsEffect::Saved(path) => gui.settings_saved(&path),
                SettingsEffect::Success(message) => gui.show_success(message),
                SettingsEffect::Error(error) => gui.show_error(error),
            }
        }
    }

    fn render_frame(
        &mut self,
        window: &Window,
        full_output: egui::FullOutput,
    ) -> Option<RenderOutcome> {
        let paint = self.paint.as_mut()?;
        let gui = self.gui.as_mut()?;
        let repaint_delay = ui::repaint_delay(&full_output);
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
                return None;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return None,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = paint
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        paint.render_to_view(&mut encoder, &view);
        let canvas_needs_redraw = paint.has_pending_stamps();

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
            gui.renderer
                .render(&mut pass, &paint_jobs, &screen_descriptor);
        }

        paint.queue().submit(
            user_cmd_bufs
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();

        for id in &full_output.textures_delta.free {
            gui.renderer.free_texture(id);
        }

        Some(RenderOutcome {
            repaint_delay,
            canvas_needs_redraw,
        })
    }

    fn apply_pending_brush_change(&mut self) -> bool {
        let Some(change) = self.settings.take_pending_brush_change() else {
            return false;
        };
        let Some(paint) = self.paint.as_mut() else {
            self.settings.restore_pending_brush_change(change);
            return false;
        };
        match paint.try_set_brush_preset(&change.brush) {
            Ok(false) => {
                self.settings.restore_pending_brush_change(change);
                false
            }
            Ok(true) => {
                let completed = self.settings.complete_brush_change(change);
                let Some(gui) = self.gui.as_mut() else {
                    return true;
                };
                gui.apply_brush_preset(self.settings.active_brush(), completed.catalog);
                if completed.reloaded {
                    gui.settings_reloaded(self.settings.config());
                }
                if !completed.warnings.is_empty() {
                    gui.show_error(completed.warnings.join("\n"));
                }
                true
            }
            Err(error) => {
                log::error!("failed to switch brush texture: {error}");
                if let Some(gui) = self.gui.as_mut() {
                    gui.show_error(error);
                }
                false
            }
        }
    }

    fn update_repaint_schedule(
        &mut self,
        repaint_delay: Duration,
        window: &Window,
        force_immediate: bool,
    ) {
        if force_immediate || repaint_delay.is_zero() {
            self.next_repaint = None;
            window.request_redraw();
        } else if repaint_delay == Duration::MAX {
            self.next_repaint = None;
        } else {
            self.next_repaint = Instant::now().checked_add(repaint_delay);
        }
    }

    fn request_scheduled_redraw(&mut self, event_loop: &ActiveEventLoop) {
        self.next_repaint = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        let Some(next_repaint) = self.next_repaint else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };

        if next_repaint <= Instant::now() {
            self.request_scheduled_redraw(event_loop);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(next_repaint));
        }
    }
}

pub fn run() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = App::new(SettingsController::load());
    event_loop.run_app(&mut app).expect("event loop error");
}
