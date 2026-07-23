use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError},
    Arc,
};

use chromazen::{
    config::{BrushCatalog, CurrentBrushConfig, LoadedBrushPreset},
    paint::{BrushSettings, BrushSpacing, PressureSettings, StrokeSmoothingOptions},
    perf::PaintPerf,
    platform::PressureStateHandle,
    protocol::{BrushUiState, UiCommand, UiMessage, UiSnapshot},
    renderer::PaintRenderer,
    settings::{SettingsCommand, SettingsController, SettingsEffect},
};
use tauri::{Emitter, EventLoopMessage, LogicalPosition, LogicalSize, Rect, Webview};
use tauri_runtime::window::WindowId as RuntimeWindowId;
use tauri_runtime_wry::{
    tao::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoopProxy, EventLoopWindowTarget},
        window::WindowId,
    },
    Context, EventLoopIterationContext, Message, Plugin, PluginBuilder, WebContextStore,
    WindowMessage,
};

use crate::{
    desktop::HistoryMenu,
    input_adapter::{InputAction, NativeInputController},
};

const PAINT_WINDOW_LABEL: &str = "main";

pub(crate) struct RawPaintPluginBuilder {
    paint: PaintRenderer,
    controls: Webview,
    controls_width: f64,
    scale_factor: f64,
    commands: Receiver<UiCommand>,
    settings: SettingsController,
    pressure_state: PressureStateHandle,
    pressure_redraw: Arc<AtomicBool>,
    history_menu: HistoryMenu,
}

impl RawPaintPluginBuilder {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        paint: PaintRenderer,
        controls: Webview,
        controls_width: f64,
        scale_factor: f64,
        commands: Receiver<UiCommand>,
        settings: SettingsController,
        pressure_state: PressureStateHandle,
        pressure_redraw: Arc<AtomicBool>,
        history_menu: HistoryMenu,
    ) -> Self {
        Self {
            paint,
            controls,
            controls_width,
            scale_factor,
            commands,
            settings,
            pressure_state,
            pressure_redraw,
            history_menu,
        }
    }
}

impl PluginBuilder<EventLoopMessage> for RawPaintPluginBuilder {
    type Plugin = RawPaintPlugin;

    fn build(mut self, _context: Context<EventLoopMessage>) -> Self::Plugin {
        let catalog = self.settings.take_startup_catalog();
        let message = self.settings.take_startup_error().map(|text| UiMessage {
            text,
            is_error: true,
        });
        let brush =
            brush_settings_from_config(self.settings.config(), self.settings.active_brush());
        let smoothing_strength = self.settings.config().smoothing.strength;
        RawPaintPlugin {
            paint: self.paint,
            controls: self.controls,
            controls_width: self.controls_width,
            scale_factor: self.scale_factor,
            tao_window_id: None,
            runtime_window_id: None,
            redraw_pending: true,
            commands: self.commands,
            settings: self.settings,
            catalog,
            message,
            input: NativeInputController::default(),
            brush,
            smoothing: StrokeSmoothingOptions {
                strength: smoothing_strength,
            },
            pressure_state: self.pressure_state,
            pressure_redraw: self.pressure_redraw,
            history_menu: self.history_menu,
            snapshot_dirty: true,
            revision: 0,
            perf: PaintPerf::default(),
        }
    }
}

pub(crate) struct RawPaintPlugin {
    paint: PaintRenderer,
    controls: Webview,
    controls_width: f64,
    scale_factor: f64,
    tao_window_id: Option<WindowId>,
    runtime_window_id: Option<RuntimeWindowId>,
    redraw_pending: bool,
    commands: Receiver<UiCommand>,
    settings: SettingsController,
    catalog: BrushCatalog,
    message: Option<UiMessage>,
    input: NativeInputController,
    brush: BrushSettings,
    smoothing: StrokeSmoothingOptions,
    pressure_state: PressureStateHandle,
    pressure_redraw: Arc<AtomicBool>,
    history_menu: HistoryMenu,
    snapshot_dirty: bool,
    revision: u64,
    perf: PaintPerf,
}

impl Plugin<EventLoopMessage> for RawPaintPlugin {
    fn on_event(
        &mut self,
        event: &Event<Message<EventLoopMessage>>,
        _event_loop: &EventLoopWindowTarget<Message<EventLoopMessage>>,
        proxy: &EventLoopProxy<Message<EventLoopMessage>>,
        _control_flow: &mut ControlFlow,
        context: EventLoopIterationContext<'_, EventLoopMessage>,
        _web_context: &WebContextStore,
    ) -> bool {
        match event {
            Event::WindowEvent {
                window_id, event, ..
            } if self.is_paint_window(*window_id, &context) => {
                self.handle_window_event(event);
            }
            Event::MainEventsCleared => {
                if self.pressure_redraw.swap(false, Ordering::Acquire) {
                    self.redraw_pending = true;
                }
                self.drain_commands();
                self.apply_pending_brush_change();
                self.emit_snapshot_if_dirty();
                if self.redraw_pending {
                    self.request_redraw(proxy);
                }
            }
            Event::RedrawRequested(window_id)
                if self.is_paint_window(*window_id, &context) && self.redraw_pending =>
            {
                let presented = self.render();
                self.redraw_pending = !presented || self.paint.has_pending_stamps();
                if self.redraw_pending {
                    self.request_redraw(proxy);
                }
            }
            _ => {}
        }
        false
    }
}

impl RawPaintPlugin {
    fn handle_window_event(&mut self, event: &WindowEvent<'_>) {
        match event {
            WindowEvent::Resized(size) => {
                self.resize([size.width, size.height]);
                self.redraw_pending = true;
            }
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                new_inner_size,
            } => {
                self.scale_factor = *scale_factor;
                self.resize([new_inner_size.width, new_inner_size.height]);
                self.redraw_pending = true;
            }
            _ => {
                let received_at = self.perf.input_received();
                let outcome = self.input.handle_event(
                    event,
                    &mut self.paint,
                    self.brush,
                    self.smoothing,
                    &self.pressure_state,
                );
                self.redraw_pending |= outcome.needs_redraw;
                self.perf.stamps_queued(
                    received_at,
                    outcome.queued_stamps,
                    outcome.pressure_sampled,
                );
                if let Some(action) = outcome.action {
                    match action {
                        InputAction::Undo => {
                            self.paint.undo();
                        }
                        InputAction::Redo => {
                            self.paint.redo();
                        }
                    }
                    self.redraw_pending = true;
                }
                if outcome.needs_redraw
                    || outcome.action.is_some()
                    || matches!(event, WindowEvent::MouseInput { .. })
                {
                    self.snapshot_dirty = true;
                }
            }
        }
    }

    fn drain_commands(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(command) => self.apply_command(command),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    }

    fn apply_command(&mut self, command: UiCommand) {
        let mut redraw = false;
        match command {
            UiCommand::RequestSnapshot => {}
            UiCommand::SetTool { tool } => {
                self.input.set_tool(tool);
            }
            UiCommand::SetBrushSize { size } => {
                if size.is_finite() {
                    let preset = &self.settings.active_brush().preset;
                    self.brush.size = size.clamp(preset.size.min, preset.size.max);
                }
            }
            UiCommand::SetBrushColor { mut color } => {
                color[3] = 255;
                self.brush.color = color;
            }
            UiCommand::SetSmoothingStrength { strength } => {
                if strength.is_finite() {
                    self.smoothing.strength = strength.clamp(0.0, 1.0);
                }
            }
            UiCommand::SelectBrush { id } => {
                self.handle_settings_command(SettingsCommand::SwitchBrush(id));
            }
            UiCommand::SelectLayer { id } => {
                self.paint.select_layer(id);
            }
            UiCommand::SelectBackground => self.paint.select_background(),
            UiCommand::AddLayer => {
                redraw = self.paint.add_layer();
            }
            UiCommand::DeleteSelectedLayer => {
                redraw = self.paint.delete_selected_layer();
            }
            UiCommand::SetBackgroundColor { color } => {
                self.paint.set_background_color(color);
                redraw = true;
            }
            UiCommand::CommitBackgroundColor { before, after } => {
                self.paint.commit_background_color(before, after);
                redraw = true;
            }
            UiCommand::FitCanvas => {
                self.paint.fit_to_screen();
                redraw = true;
            }
            UiCommand::Undo => {
                redraw = self.paint.undo();
            }
            UiCommand::Redo => {
                redraw = self.paint.redo();
            }
            UiCommand::SaveSettings => {
                self.handle_settings_command(SettingsCommand::Save {
                    brush: CurrentBrushConfig {
                        size: self.brush.size,
                        color: self.brush.color,
                    },
                    active_brush: self.settings.active_brush().id.clone(),
                });
            }
            UiCommand::ReloadConfiguration => {
                self.handle_settings_command(SettingsCommand::ReloadFromDisk);
            }
            UiCommand::ResetBrush => {
                self.brush.size = self.settings.active_brush().preset.size.default;
                self.brush.color = CurrentBrushConfig::default().color;
                self.message = None;
            }
        }
        self.redraw_pending |= redraw;
        self.snapshot_dirty = true;
    }

    fn handle_settings_command(&mut self, command: SettingsCommand) {
        let Some(effect) = self.settings.handle_command(command) else {
            return;
        };
        self.message = Some(match effect {
            SettingsEffect::Success(text) => UiMessage {
                text,
                is_error: false,
            },
            SettingsEffect::Error(text) => UiMessage {
                text,
                is_error: true,
            },
        });
    }

    fn apply_pending_brush_change(&mut self) {
        let Some(change) = self.settings.take_pending_brush_change() else {
            return;
        };
        match self.paint.try_set_brush_preset(&change.brush) {
            Ok(false) => self.settings.restore_pending_brush_change(change),
            Ok(true) => {
                let completed = self.settings.complete_brush_change(change);
                self.catalog = completed.catalog;
                apply_brush_preset(&mut self.brush, self.settings.active_brush());
                if completed.reloaded {
                    let config = self.settings.config();
                    self.brush.color = config.brush.color;
                    self.brush.size = config.brush.size.clamp(
                        self.settings.active_brush().preset.size.min,
                        self.settings.active_brush().preset.size.max,
                    );
                    self.smoothing.strength = config.smoothing.strength;
                }
                if completed.warnings.is_empty() {
                    self.message = None;
                } else {
                    self.message = Some(UiMessage {
                        text: completed.warnings.join("\n"),
                        is_error: true,
                    });
                }
                self.redraw_pending = true;
                self.snapshot_dirty = true;
            }
            Err(error) => {
                log::error!("failed to switch brush texture: {error}");
                self.message = Some(UiMessage {
                    text: error,
                    is_error: true,
                });
                self.snapshot_dirty = true;
            }
        }
    }

    fn emit_snapshot_if_dirty(&mut self) {
        if !self.snapshot_dirty {
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        let preset = &self.settings.active_brush().preset;
        let snapshot = UiSnapshot {
            revision: self.revision,
            tool: self.input.tool(),
            brush: BrushUiState {
                size: self.brush.size,
                color: self.brush.color,
                minimum_size: preset.size.min,
                maximum_size: preset.size.max,
                default_size: preset.size.default,
            },
            smoothing_strength: self.smoothing.strength,
            active_brush: self.settings.active_brush().id.clone(),
            brushes: self.catalog.brushes.clone(),
            layers: self.paint.layer_snapshot(),
            can_undo: self.paint.can_undo(),
            can_redo: self.paint.can_redo(),
            can_delete_layer: self.paint.can_delete_selected_layer(),
            message: self.message.clone(),
        };
        self.history_menu
            .set_enabled(snapshot.can_undo, snapshot.can_redo);
        match self.controls.emit("ui-state", &snapshot) {
            Ok(()) => self.snapshot_dirty = false,
            Err(error) => log::warn!("failed to emit control snapshot: {error}"),
        }
    }

    fn is_paint_window(
        &mut self,
        window_id: WindowId,
        context: &EventLoopIterationContext<'_, EventLoopMessage>,
    ) -> bool {
        if let Some(paint_window_id) = self.tao_window_id {
            return paint_window_id == window_id;
        }

        let Some(runtime_id) = context.window_id_map.get(&window_id) else {
            return false;
        };
        let windows = context.windows.0.borrow();
        let is_paint_window = windows
            .get(&runtime_id)
            .is_some_and(|window| window.label() == PAINT_WINDOW_LABEL);
        if is_paint_window {
            self.tao_window_id = Some(window_id);
            self.runtime_window_id = Some(runtime_id);
        }
        is_paint_window
    }

    fn resize(&mut self, size: [u32; 2]) {
        self.paint.resize(size);
        let logical_width = size[0] as f64 / self.scale_factor;
        let logical_height = size[1] as f64 / self.scale_factor;
        let controls_width = self.controls_width.min(logical_width);
        if let Err(error) = self.controls.set_bounds(Rect {
            position: LogicalPosition::new(logical_width - controls_width, 0.0).into(),
            size: LogicalSize::new(controls_width, logical_height).into(),
        }) {
            log::warn!("failed to resize controls webview: {error}");
        }
        self.paint.set_canvas_viewport_size([
            canvas_viewport_width(size[0], self.controls_width, self.scale_factor),
            size[1],
        ]);
    }

    fn request_redraw(&self, proxy: &EventLoopProxy<Message<EventLoopMessage>>) {
        let Some(window_id) = self.runtime_window_id else {
            return;
        };
        if let Err(error) =
            proxy.send_event(Message::Window(window_id, WindowMessage::RequestRedraw))
        {
            log::warn!("failed to request native paint redraw: {error}");
        }
    }

    fn render(&mut self) -> bool {
        let frame = match self.paint.acquire_frame() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.paint.reconfigure_surface();
                return false;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return false,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.paint
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("tauri native paint frame"),
                });
        self.paint.render_to_view(&mut encoder, &view);
        self.paint.queue().submit(std::iter::once(encoder.finish()));
        self.perf.submitted();
        frame.present();
        self.perf.presented();
        true
    }
}

fn brush_settings_from_config(
    config: &chromazen::config::AppConfig,
    preset: &LoadedBrushPreset,
) -> BrushSettings {
    let mut brush = BrushSettings::default();
    apply_brush_preset(&mut brush, preset);
    brush.color = config.brush.color;
    brush.size = config
        .brush
        .size
        .clamp(preset.preset.size.min, preset.preset.size.max);
    brush
}

fn apply_brush_preset(brush: &mut BrushSettings, loaded: &LoadedBrushPreset) {
    let preset = &loaded.preset;
    brush.size = preset.size.default;
    brush.pressure = PressureSettings {
        min_size: preset.pressure.min_size,
        min_opacity: preset.pressure.min_opacity,
        opacity_gamma: preset.pressure.opacity_gamma,
    };
    brush.spacing = BrushSpacing {
        ratio: preset.spacing.ratio,
        minimum: preset.spacing.minimum,
    };
}

fn canvas_viewport_width(surface_width: u32, controls_width: f64, scale_factor: f64) -> u32 {
    let controls_width = (controls_width * scale_factor).round() as u32;
    surface_width.saturating_sub(controls_width).max(1)
}

#[cfg(test)]
mod tests {
    use super::canvas_viewport_width;

    #[test]
    fn viewport_excludes_physical_controls_width() {
        assert_eq!(canvas_viewport_width(1_280, 300.0, 1.0), 980);
        assert_eq!(canvas_viewport_width(2_560, 300.0, 2.0), 1_960);
    }

    #[test]
    fn viewport_never_becomes_zero() {
        assert_eq!(canvas_viewport_width(200, 300.0, 1.0), 1);
    }
}
