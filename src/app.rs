mod input;
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
    ui::{GuiAction, GuiLayer},
};
use crate::{
    config::{AppConfig, BrushCatalog, ConfigStore, LoadedBrushPreset},
    platform::{MacosPressureMonitor, PressureStateHandle},
    renderer::PaintRenderer,
};

const WINDOW_TITLE: &str = "minipaint-rs";

pub struct App {
    window: Option<Arc<Window>>,
    paint: Option<PaintRenderer>,
    gui: Option<GuiLayer>,
    input: PaintInputController,
    pressure_state: PressureStateHandle,
    _pressure_monitor: Option<MacosPressureMonitor>,
    next_repaint: Option<Instant>,
    config_store: Option<ConfigStore>,
    config: AppConfig,
    brush_preset: LoadedBrushPreset,
    brush_catalog: Option<BrushCatalog>,
    pending_brush_change: Option<PendingBrushChange>,
    config_load_error: Option<String>,
}

struct PendingBrushChange {
    brush: LoadedBrushPreset,
    reloaded_config: Option<AppConfig>,
    warning: Option<String>,
}

impl PendingBrushChange {
    fn switch(brush: LoadedBrushPreset) -> Self {
        Self {
            brush,
            reloaded_config: None,
            warning: None,
        }
    }

    fn reload(mut config: AppConfig, brush: LoadedBrushPreset, warning: Option<String>) -> Self {
        normalize_active_brush(&mut config, &brush);
        Self {
            brush,
            reloaded_config: Some(config),
            warning,
        }
    }
}

fn normalize_active_brush(config: &mut AppConfig, brush: &LoadedBrushPreset) {
    config.active_brush.clone_from(&brush.id);
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
        let paint = pollster::block_on(PaintRenderer::new(window.clone(), &self.brush_preset))
            .expect("failed to initialize wgpu paint renderer");
        let gui = GuiLayer::new(
            window.as_ref(),
            &paint,
            &self.config,
            &self.brush_preset,
            self.brush_catalog.take().unwrap_or_default(),
            self.config_load_error.take(),
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
    fn new(
        config_store: Option<ConfigStore>,
        config: AppConfig,
        brush_preset: LoadedBrushPreset,
        brush_catalog: BrushCatalog,
        config_load_error: Option<String>,
    ) -> Self {
        Self {
            window: None,
            paint: None,
            gui: None,
            input: PaintInputController::default(),
            pressure_state: PressureStateHandle::default(),
            _pressure_monitor: None,
            next_repaint: None,
            config_store,
            config,
            brush_preset,
            brush_catalog: Some(brush_catalog),
            pending_brush_change: None,
            config_load_error,
        }
    }

    fn render(&mut self, window: &Window) {
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        let Some(gui) = self.gui.as_mut() else {
            return;
        };
        if paint.surface_size()[0] == 0 || paint.surface_size()[1] == 0 {
            return;
        }

        let full_output = gui.run(window);
        let mut settings_action_processed = gui.take_save_requested();
        if settings_action_processed {
            self.config.brush = gui.current_brush_config();
            self.config.active_brush = gui.active_brush().to_owned();
            if let Some(store) = &self.config_store {
                match store.save_app_config(&self.config) {
                    Ok(()) => gui.settings_saved(&store.config_path()),
                    Err(error) => {
                        log::error!("failed to save settings: {error}");
                        gui.settings_save_failed(error.to_string());
                    }
                }
            } else {
                gui.settings_save_failed("The configuration directory is unavailable");
            }
        }

        if let Some(action) = gui.take_action() {
            settings_action_processed = true;
            let result = match (&self.config_store, action) {
                (Some(store), GuiAction::SwitchBrush(id)) => store.load_brush(&id).map(|brush| {
                    self.pending_brush_change = Some(PendingBrushChange::switch(brush));
                }),
                (Some(store), GuiAction::ReloadFromDisk) => {
                    store.load_app_config().map(|config| {
                        let (brush, warning) = match store.load_brush(&config.active_brush) {
                            Ok(brush) => (brush, None),
                            Err(error) => {
                                let warning = format!(
                                    "Could not reload brush '{}': {error}. Using bundled Charcoal instead.",
                                    config.active_brush
                                );
                                (LoadedBrushPreset::bundled_charcoal(), Some(warning))
                            }
                        };
                        self.pending_brush_change =
                            Some(PendingBrushChange::reload(config, brush, warning));
                    })
                },
                (Some(store), GuiAction::OpenConfigDirectory) => store
                    .open_config_directory()
                    .map(|()| gui.show_success("Opened the configuration folder")),
                (None, _) => Err(crate::config::ConfigError::unavailable()),
            };
            if let Err(error) = result {
                log::error!("brush preset action failed: {error}");
                gui.show_error(error.to_string());
            }
        }
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
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
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

        let mut brush_switched = false;
        if !canvas_needs_redraw && let Some(change) = self.pending_brush_change.take() {
            let PendingBrushChange {
                brush: loaded,
                reloaded_config,
                warning: reload_warning,
            } = change;
            match paint.set_brush_preset(&loaded) {
                Ok(()) => {
                    let catalog = self
                        .config_store
                        .as_ref()
                        .map_or_else(BrushCatalog::default, ConfigStore::discover_brushes);
                    let mut warnings = catalog.warnings.clone();
                    if let Some(warning) = reload_warning {
                        log::warn!("{warning}");
                        warnings.insert(0, warning);
                    }
                    for warning in &catalog.warnings {
                        log::warn!("failed to discover brush: {warning}");
                    }
                    let combined_warning = (!warnings.is_empty()).then(|| warnings.join("\n"));
                    self.config.active_brush.clone_from(&loaded.id);
                    gui.apply_brush_preset(&loaded, catalog);
                    if let Some(config) = reloaded_config {
                        self.config = config;
                        gui.settings_reloaded(&self.config);
                    }
                    if let Some(warning) = combined_warning {
                        gui.show_error(warning);
                    }
                    self.brush_preset = loaded;
                    brush_switched = true;
                }
                Err(error) => {
                    log::error!("failed to switch brush texture: {error}");
                    gui.show_error(error);
                }
            }
        }

        self.update_repaint_schedule(
            repaint_delay,
            window,
            canvas_needs_redraw || settings_action_processed || brush_switched,
        );
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
    let (config_store, mut config, mut config_load_error) = match ConfigStore::discover() {
        Ok(store) => match store.load_app_config() {
            Ok(config) => (Some(store), config, None),
            Err(error) => {
                log::error!("failed to load settings: {error}");
                (Some(store), AppConfig::default(), Some(error.to_string()))
            }
        },
        Err(error) => {
            log::error!("failed to locate settings: {error}");
            (None, AppConfig::default(), Some(error.to_string()))
        }
    };

    let brush_catalog = config_store
        .as_ref()
        .map_or_else(BrushCatalog::default, ConfigStore::discover_brushes);
    for warning in &brush_catalog.warnings {
        log::warn!("failed to discover brush: {warning}");
    }
    if !brush_catalog.warnings.is_empty() {
        let warning = format!(
            "Some brush presets could not be loaded:\n{}",
            brush_catalog.warnings.join("\n")
        );
        config_load_error = Some(match config_load_error {
            Some(existing) => format!("{existing}\n{warning}"),
            None => warning,
        });
    }
    log::debug!("discovered {} brush preset(s)", brush_catalog.brushes.len());

    let brush_preset = if let Some(store) = &config_store {
        match store.load_brush(&config.active_brush) {
            Ok(brush) => brush,
            Err(error) => {
                log::error!("failed to load brush preset: {error}");
                let message = format!("Could not load brush '{}': {error}", config.active_brush);
                config_load_error = Some(match config_load_error {
                    Some(existing) => format!("{existing}\n{message}"),
                    None => message,
                });
                LoadedBrushPreset::bundled_charcoal()
            }
        }
    } else {
        LoadedBrushPreset::bundled_charcoal()
    };
    normalize_active_brush(&mut config, &brush_preset);

    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = App::new(
        config_store,
        config,
        brush_preset,
        brush_catalog,
        config_load_error,
    );
    event_loop.run_app(&mut app).expect("event loop error");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_normalizes_missing_brush_to_effective_fallback() {
        let config = AppConfig {
            active_brush: "missing".to_owned(),
            ..AppConfig::default()
        };

        let change = PendingBrushChange::reload(
            config,
            LoadedBrushPreset::bundled_charcoal(),
            Some("missing brush".to_owned()),
        );

        assert_eq!(
            change
                .reloaded_config
                .expect("reloaded config")
                .active_brush,
            change.brush.id
        );
    }
}
