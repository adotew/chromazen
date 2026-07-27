mod autosave;
mod command;
mod gallery;
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
    event::{ElementState, MouseButton, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{CursorIcon, Window, WindowAttributes},
};

use self::{
    autosave::AutosaveController,
    command::AppCommand,
    gallery::GalleryController,
    input::{KeyboardShortcut, PaintInputController},
    menu::NativeMenu,
    settings::{SettingsCommand, SettingsController, SettingsEffect},
    ui::{BrushResizeLabel, EyedropperIndicator, GuiLayer},
};
use crate::{
    platform::{MacosPressureMonitor, PressureStateHandle},
    renderer::{BrushCursor, PaintRenderer},
};

const WINDOW_TITLE: &str = "Chromazen";

enum AppEvent {
    Command(AppCommand),
    AutosaveWake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppScreen {
    Gallery,
    Editor,
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
    gallery: GalleryController,
    autosave: AutosaveController,
    screen: AppScreen,
    pending_gallery: bool,
    pending_exit: bool,
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
            WindowEvent::CloseRequested => self.request_exit(event_loop),
            WindowEvent::RedrawRequested => self.render(window.as_ref(), event_loop),
            event => {
                let navigation_pending = self.navigation_pending();
                let Some(gui) = self.gui.as_mut() else {
                    return;
                };
                let cursor_changed = self.input.observe_event(&event);
                if self.screen == AppScreen::Editor
                    && !navigation_pending
                    && let Some(shortcut) = self.input.keyboard_shortcut(&event)
                {
                    let changed = match shortcut {
                        KeyboardShortcut::ToggleSidebar => {
                            gui.toggle_sidebar();
                            true
                        }
                        KeyboardShortcut::CycleTool => self.input.cycle_tool(),
                    };
                    if changed {
                        self.next_repaint = None;
                        window.request_redraw();
                    }
                    return;
                }
                let egui_response = gui.state.on_window_event(window.as_ref(), &event);
                let mut needs_redraw = egui_response.repaint || cursor_changed;
                let egui_consumed = egui_response.consumed;
                if !egui_consumed
                    && matches!(
                        &event,
                        WindowEvent::MouseInput {
                            state: ElementState::Pressed,
                            button: MouseButton::Left,
                            ..
                        }
                    )
                {
                    needs_redraw |= gui.close_popups();
                }

                let history_command =
                    (self.screen == AppScreen::Editor && !navigation_pending && !egui_consumed)
                        .then(|| self.input.app_command(&event))
                        .flatten();
                if let Some(command) = history_command {
                    self.pending_commands.push(command);
                    needs_redraw = true;
                } else if self.screen == AppScreen::Editor
                    && !navigation_pending
                    && (!egui_consumed || self.input.captures_drag_event(&event))
                    && let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_mut())
                {
                    let brush_size_range = gui.brush_size_range();
                    needs_redraw |= self.input.handle_event(
                        &event,
                        paint,
                        &mut gui.brush,
                        brush_size_range,
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

                self.sync_history_menu();
                if needs_redraw {
                    self.next_repaint = None;
                    window.request_redraw();
                }
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Command(command) => self.pending_commands.push(command),
            AppEvent::AutosaveWake => {}
        }
        self.next_repaint = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.request_scheduled_redraw(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.update_control_flow(event_loop);
    }
}

impl App {
    fn new(
        settings: SettingsController,
        native_menu: NativeMenu,
        gallery: GalleryController,
        autosave: AutosaveController,
    ) -> Self {
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
            gallery,
            autosave,
            screen: AppScreen::Gallery,
            pending_gallery: false,
            pending_exit: false,
        }
    }

    fn navigation_pending(&self) -> bool {
        self.pending_gallery || self.pending_exit
    }

    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
        if self.screen == AppScreen::Gallery {
            event_loop.exit();
            return;
        }
        if let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref()) {
            self.input.finish_document_interaction(paint, gui.brush);
        }
        let clean = self
            .paint
            .as_ref()
            .is_some_and(|paint| self.autosave.is_clean(paint));
        if clean {
            event_loop.exit();
            return;
        }
        self.pending_exit = true;
        self.pending_gallery = false;
        self.autosave.request_save();
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn render(&mut self, window: &Window, event_loop: &ActiveEventLoop) {
        let mut app_action_processed = self.process_pending_commands();
        let mut brush_switched = self.apply_pending_brush_change();

        if self.screen == AppScreen::Editor
            && let Some(paint) = self.paint.as_ref()
        {
            app_action_processed |= self.autosave.update(paint);
            if self.pending_exit && self.autosave.is_clean(paint) {
                event_loop.exit();
                return;
            }
            if self.pending_gallery && self.autosave.is_clean(paint) {
                self.finish_gallery_navigation();
                app_action_processed = true;
            }
        }

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
            let output = match self.screen {
                AppScreen::Gallery => {
                    let warning = self.gallery.warning();
                    gui.run_gallery(window, self.gallery.artworks(), warning.as_deref())
                }
                AppScreen::Editor => {
                    gui.sync_layer_thumbnails(paint);
                    let layer_snapshot = paint.layer_snapshot();
                    let brush_resize_label =
                        self.input
                            .brush_resize_pos()
                            .map(|center| BrushResizeLabel {
                                center,
                                outline_half_width: paint.brush_outline_half_size(gui.brush.size)
                                    [0],
                            });
                    let eyedropper_indicator =
                        self.input
                            .eyedropper_indicator_pos()
                            .map(|center| EyedropperIndicator {
                                center,
                                color: gui.brush.color,
                            });
                    let title = self.autosave.title().unwrap_or("Untitled");
                    let status = self.autosave.status(paint);
                    let pending_navigation = if self.pending_exit {
                        Some("Closing Chromazen")
                    } else if self.pending_gallery {
                        Some("Returning to Gallery")
                    } else {
                        None
                    };
                    gui.run_editor(
                        window,
                        &layer_snapshot,
                        self.input.tool(),
                        brush_resize_label,
                        eyedropper_indicator,
                        title,
                        status,
                        pending_navigation,
                    )
                }
            };
            (output, gui.take_commands())
        };
        self.pending_commands.extend(commands);
        app_action_processed |= self.process_pending_commands();

        if self.screen == AppScreen::Editor
            && let Some(paint) = self.paint.as_ref()
        {
            app_action_processed |= self.autosave.update(paint);
        }
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
                AppCommand::SelectTool(tool) => {
                    self.input.select_tool(tool);
                }
                AppCommand::SelectLayer(id) => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.select_layer(id);
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
                AppCommand::NewArtwork => self.new_artwork(),
                AppCommand::OpenArtwork(id) => self.open_artwork(&id),
                AppCommand::SaveArtwork => {
                    if self.screen == AppScreen::Editor {
                        self.autosave.request_save();
                    }
                }
                AppCommand::ShowGallery => {
                    if self.screen == AppScreen::Editor {
                        self.pending_gallery = true;
                        self.autosave.request_save();
                    }
                }
                AppCommand::RenameArtwork { id, title } => {
                    if let Err(error) = self.gallery.rename(&id, &title)
                        && let Some(gui) = self.gui.as_mut()
                    {
                        gui.show_error(error);
                    }
                }
                AppCommand::DeleteArtwork(id) => {
                    if let Err(error) = self.gallery.delete(&id)
                        && let Some(gui) = self.gui.as_mut()
                    {
                        gui.show_error(error);
                    }
                }
                AppCommand::CancelPendingNavigation => {
                    self.pending_gallery = false;
                    self.pending_exit = false;
                }
            }
        }
        self.sync_history_menu();
        true
    }

    fn new_artwork(&mut self) {
        if self.screen == AppScreen::Editor {
            self.pending_gallery = true;
            self.autosave.request_save();
            return;
        }
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if !paint.reset_document() {
            return;
        }
        let id = crate::artwork::ArtworkId::new();
        self.autosave.begin_new(id, "Untitled".to_owned());
        self.screen = AppScreen::Editor;
        self.pending_gallery = false;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title("Untitled — Chromazen");
        }
    }

    fn open_artwork(&mut self, id: &crate::artwork::ArtworkId) {
        let opened = match self.gallery.open(id) {
            Ok(opened) => opened,
            Err(error) => {
                if let Some(gui) = self.gui.as_mut() {
                    gui.show_error(error);
                }
                return;
            }
        };
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if let Err(error) = paint.load_document(&opened.document, opened.layers) {
            if let Some(gui) = self.gui.as_mut() {
                gui.show_error(error);
            }
            return;
        }
        let versions = paint.document_versions();
        self.autosave
            .begin_loaded(opened.id, opened.title.clone(), versions);
        self.screen = AppScreen::Editor;
        self.pending_gallery = false;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(&format!("{} — Chromazen", opened.title));
        }
    }

    fn finish_gallery_navigation(&mut self) {
        self.gallery.refresh();
        self.autosave.clear();
        self.screen = AppScreen::Gallery;
        self.pending_gallery = false;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(WINDOW_TITLE);
        }
        self.sync_history_menu();
    }

    fn sync_history_menu(&self) {
        let (can_undo, can_redo) = (self.screen == AppScreen::Editor)
            .then(|| self.paint.as_ref())
            .flatten()
            .map_or((false, false), |paint| (paint.can_undo(), paint.can_redo()));
        self.native_menu.set_history_enabled(can_undo, can_redo);
        self.native_menu
            .set_document_enabled(self.screen == AppScreen::Editor);
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
        let cursor_pos = self.input.brush_cursor_pos();
        let brush_resize_pos = self.input.brush_resize_pos();
        let resize_is_anchored = self.input.brush_resize_is_anchored();
        let is_resizing_brush = self.input.is_resizing_brush();
        let is_panning = self.input.is_panning();
        let is_pan_modifier_active = self.input.is_pan_modifier_active();
        let is_eyedropper_active = self.input.is_eyedropper_active();
        let brush_pressure = self.pressure_state.brush_pressure();
        let paint = self.paint.as_mut()?;
        let gui = self.gui.as_mut()?;
        let pointer_over_ui = gui.context.is_pointer_over_egui();
        let brush_cursor = brush_resize_pos
            .filter(|_| resize_is_anchored || !pointer_over_ui)
            .map(|center| BrushCursor {
                center,
                diameter: gui.brush.size,
            })
            .or_else(|| {
                cursor_pos
                    .filter(|_| !pointer_over_ui)
                    .map(|center| BrushCursor {
                        center,
                        diameter: gui.brush.radius(brush_pressure) * 2.0,
                    })
            });
        let repaint_delay = ui::repaint_delay(&full_output);
        gui.state
            .handle_platform_output(window, full_output.platform_output);
        if is_panning {
            window.set_cursor(CursorIcon::Grabbing);
        } else if is_pan_modifier_active && !pointer_over_ui {
            window.set_cursor(CursorIcon::Grab);
        }
        let eyedropper_over_canvas = is_eyedropper_active && !pointer_over_ui;
        window.set_cursor_visible(
            is_resizing_brush || (brush_cursor.is_none() && !eyedropper_over_canvas),
        );

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

        paint.render_to_view(&mut encoder, &view, brush_cursor);
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
                gui.apply_brush_preset(
                    self.settings.active_brush(),
                    completed.catalog,
                    completed.reloaded,
                );
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
        let next_repaint = match (
            self.next_repaint,
            (self.screen == AppScreen::Editor)
                .then(|| self.autosave.next_deadline())
                .flatten(),
        ) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        let Some(next_repaint) = next_repaint else {
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
    let menu_proxy = proxy.clone();
    native_menu.set_event_handler(move |command| {
        if menu_proxy.send_event(AppEvent::Command(command)).is_err() {
            log::debug!("native menu event ignored after event loop shutdown");
        }
    });
    let gallery = GalleryController::discover();
    let autosave_store = gallery.store();
    let wake = Arc::new(move || {
        let _ = proxy.send_event(AppEvent::AutosaveWake);
    });
    let autosave = AutosaveController::new(autosave_store, wake);

    let mut app = App::new(SettingsController::load(), native_menu, gallery, autosave);
    event_loop.run_app(&mut app).expect("event loop error");
}
