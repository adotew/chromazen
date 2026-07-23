mod command;
mod input;
mod menu;
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
    command::AppCommand,
    input::PaintInputController,
    menu::NativeMenu,
    settings::{SettingsCommand, SettingsController, SettingsEffect},
    ui::GuiLayer,
};
use crate::{
    perf::PaintPerf,
    platform::{MacosPressureMonitor, PressureStateHandle},
    renderer::PaintRenderer,
};

const WINDOW_TITLE: &str = "Chromazen";

enum AppEvent {
    Command(AppCommand),
}

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
    pending_commands: Vec<AppCommand>,
    settings: SettingsController,
    native_menu: NativeMenu,
    perf: PaintPerf,
}

impl ApplicationHandler<AppEvent> for App {
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

        self.native_menu
            .install(window.as_ref())
            .unwrap_or_else(|error| panic!("failed to install native menu: {error}"));

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
        self.sync_history_menu();
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
                self.input.observe_event(&event);
                let egui_response = gui.state.on_window_event(window.as_ref(), &event);
                let mut needs_redraw = egui_response.repaint;
                let egui_consumed = egui_response.consumed;

                if !egui_consumed {
                    let received_at = self.perf.input_received();
                    if let Some(command) = self.input.history_command(&event) {
                        self.pending_commands.push(command);
                        needs_redraw = true;
                    } else if let (Some(paint), Some(gui)) =
                        (self.paint.as_mut(), self.gui.as_ref())
                    {
                        let outcome = self.input.handle_event(
                            &event,
                            paint,
                            gui.brush,
                            gui.stroke_smoothing,
                            &self.pressure_state,
                        );
                        needs_redraw |= outcome.needs_redraw;
                        self.perf.stamps_queued(
                            received_at,
                            outcome.queued_stamps,
                            outcome.pressure_sampled,
                        );
                    }
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

                self.sync_history_menu();
                if needs_redraw {
                    self.next_repaint = None;
                    window.request_redraw();
                }
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        let AppEvent::Command(command) = event;
        self.pending_commands.push(command);
        self.next_repaint = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
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
    fn new(settings: SettingsController, native_menu: NativeMenu) -> Self {
        Self {
            window: None,
            paint: None,
            gui: None,
            input: PaintInputController::default(),
            pressure_state: PressureStateHandle::default(),
            _pressure_monitor: None,
            next_repaint: None,
            pending_commands: Vec::new(),
            settings,
            native_menu,
            perf: PaintPerf::default(),
        }
    }

    fn render(&mut self, window: &Window) {
        let mut app_action_processed = self.process_pending_commands();
        let mut brush_switched = self.apply_pending_brush_change();

        let Some(paint) = self.paint.as_ref() else {
            return;
        };
        if paint.surface_size()[0] == 0 || paint.surface_size()[1] == 0 {
            return;
        }

        let layer_snapshot = paint.layer_snapshot();
        let tool = self.input.tool();
        let (full_output, commands) = {
            let Some(gui) = self.gui.as_mut() else {
                return;
            };
            let output = gui.run(window, &layer_snapshot, tool);
            (output, gui.take_commands())
        };
        self.pending_commands.extend(commands);
        app_action_processed |= self.process_pending_commands();

        let Some(outcome) = self.render_frame(window, full_output) else {
            return;
        };
        brush_switched |= self.apply_pending_brush_change();
        self.update_repaint_schedule(
            outcome.repaint_delay,
            window,
            outcome.canvas_needs_redraw || app_action_processed || brush_switched,
        );
    }

    fn process_pending_commands(&mut self) -> bool {
        if self.gui.is_none() || self.pending_commands.is_empty() {
            return false;
        }

        let commands = std::mem::take(&mut self.pending_commands);
        for command in commands {
            match command {
                AppCommand::Undo => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.undo();
                    }
                }
                AppCommand::Redo => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.redo();
                    }
                }
                AppCommand::SelectLayer(id) => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.select_layer(id);
                    }
                }
                AppCommand::SelectBackground => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.select_background();
                    }
                }
                AppCommand::AddLayer => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.add_layer();
                    }
                }
                AppCommand::DeleteSelectedLayer => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.delete_selected_layer();
                    }
                }
                AppCommand::SetBackgroundColor(color) => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.set_background_color(color);
                    }
                }
                AppCommand::CommitBackgroundColor { before, after } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.commit_background_color(before, after);
                    }
                }
                AppCommand::SwitchBrush(id) => {
                    self.process_settings_commands(vec![SettingsCommand::SwitchBrush(id)]);
                }
                AppCommand::SaveSettings => {
                    let Some((brush, active_brush)) =
                        self.gui.as_ref().map(GuiLayer::settings_snapshot)
                    else {
                        continue;
                    };
                    self.process_settings_commands(vec![SettingsCommand::Save {
                        brush,
                        active_brush,
                    }]);
                }
                AppCommand::ReloadConfiguration => {
                    self.process_settings_commands(vec![SettingsCommand::ReloadFromDisk]);
                }
                AppCommand::ResetBrush => {
                    if let Some(gui) = self.gui.as_mut() {
                        gui.reset_brush();
                    }
                }
                AppCommand::OpenConfigDirectory => {
                    self.process_settings_commands(vec![SettingsCommand::OpenConfigDirectory]);
                }
            }
        }
        self.sync_history_menu();
        true
    }

    fn sync_history_menu(&self) {
        let (can_undo, can_redo) = self
            .paint
            .as_ref()
            .map_or((false, false), |paint| (paint.can_undo(), paint.can_redo()));
        self.native_menu.set_history_enabled(can_undo, can_redo);
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
        self.perf.submitted();
        frame.present();
        self.perf.presented();

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
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    let native_menu =
        NativeMenu::new().unwrap_or_else(|error| panic!("failed to create native menu: {error}"));
    let proxy = event_loop.create_proxy();
    native_menu.set_event_handler(move |command| {
        if proxy.send_event(AppEvent::Command(command)).is_err() {
            log::debug!("native menu event ignored after event loop shutdown");
        }
    });

    let mut app = App::new(SettingsController::load(), native_menu);
    event_loop.run_app(&mut app).expect("event loop error");
}
