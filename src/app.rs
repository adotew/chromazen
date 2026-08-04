mod autosave;
mod brush_import;
mod command;
mod export;
mod gallery;
mod input;
mod menu;
mod reference_import;
mod reference_load;
mod references;
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
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    window::{CursorIcon, Window, WindowAttributes},
};

use self::{
    autosave::AutosaveController,
    brush_import::{BrushImportController, choose_abr_paths},
    command::AppCommand,
    export::{ExportController, choose_export_path},
    gallery::GalleryController,
    input::{KeyboardShortcut, PaintInputController},
    menu::NativeMenu,
    reference_import::{ReferenceImportController, choose_reference_paths},
    reference_load::ReferenceLoadController,
    references::ReferenceBoard,
    settings::{SettingsCommand, SettingsController, SettingsEffect},
    ui::{BrushResizeLabel, EditorUiState, EyedropperIndicator, GuiLayer},
};
use crate::{
    platform::{
        MacosPressureMonitor, PenEvent, PressureStateHandle, WaylandTabletMonitor,
        WindowsPenMonitor, WindowsPenRouter,
    },
    renderer::{BrushCursor, DocumentVersions, PaintRenderer},
};

const WINDOW_TITLE: &str = "Chromazen";

enum AppEvent {
    Command(AppCommand),
    #[cfg_attr(not(any(target_os = "linux", target_os = "windows")), allow(dead_code))]
    Pen(PenEvent),
    AutosaveWake,
    BrushImportWake,
    ExportWake,
    ReferenceImportWake,
    ReferenceLoadWake,
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

struct PendingReferenceLoad {
    id: crate::artwork::ArtworkId,
    title: String,
    paint_versions: DocumentVersions,
    brush_color: [u8; 4],
}

struct ImportControllers {
    brush: BrushImportController,
    reference: ReferenceImportController,
    reference_load: ReferenceLoadController,
}

struct PenControllers {
    proxy: EventLoopProxy<AppEvent>,
    windows_router: WindowsPenRouter,
}

pub struct App {
    window: Option<Arc<Window>>,
    paint: Option<PaintRenderer>,
    gui: Option<GuiLayer>,
    input: PaintInputController,
    pressure_state: PressureStateHandle,
    _pressure_monitor: Option<MacosPressureMonitor>,
    tablet_monitor: Option<WaylandTabletMonitor>,
    windows_pen_monitor: Option<WindowsPenMonitor>,
    windows_pen_router: WindowsPenRouter,
    pen_proxy: EventLoopProxy<AppEvent>,
    next_repaint: Option<Instant>,
    pending_commands: Vec<AppCommand>,
    settings: SettingsController,
    native_menu: NativeMenu,
    gallery: GalleryController,
    autosave: AutosaveController,
    export: ExportController,
    brush_import: BrushImportController,
    references: ReferenceBoard,
    reference_import: ReferenceImportController,
    reference_load: ReferenceLoadController,
    pending_reference_load: Option<PendingReferenceLoad>,
    screen: AppScreen,
    pending_gallery: bool,
    pending_new_artwork: Option<[u32; 2]>,
    pending_exit: bool,
}

impl Drop for App {
    fn drop(&mut self) {
        // Platform monitors must release native resources before the window.
        self.tablet_monitor.take();
        self.windows_pen_monitor.take();
    }
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
        let pen_proxy = self.pen_proxy.clone();
        let tablet_monitor = WaylandTabletMonitor::install(window.clone(), move |event| {
            if pen_proxy.send_event(AppEvent::Pen(event)).is_err() {
                log::debug!("tablet event ignored after event loop shutdown");
            }
        })
        .unwrap_or_else(|error| {
            log::warn!("Wayland tablet input unavailable: {error}");
            None
        });
        let windows_pen_proxy = self.pen_proxy.clone();
        let windows_pen_monitor = WindowsPenMonitor::install(
            window.clone(),
            self.windows_pen_router.clone(),
            move |event| {
                if windows_pen_proxy.send_event(AppEvent::Pen(event)).is_err() {
                    log::debug!("Windows pen event ignored after event loop shutdown");
                }
            },
        )
        .unwrap_or_else(|error| {
            log::warn!("Windows pen input unavailable: {error}");
            None
        });
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
        self.tablet_monitor = tablet_monitor;
        self.windows_pen_monitor = windows_pen_monitor;
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
            WindowEvent::CloseRequested => self.request_exit(),
            WindowEvent::RedrawRequested => self.render(window.as_ref(), event_loop),
            WindowEvent::DroppedFile(path)
                if self.screen == AppScreen::Editor
                    && !self.navigation_pending()
                    && !self.reference_load.is_loading() =>
            {
                let placement = self
                    .paint
                    .as_ref()
                    .map(|paint| paint.window_to_document(self.input.cursor_position()));
                if let Some(artwork_id) = self.autosave.artwork_id().cloned() {
                    self.reference_import
                        .start(artwork_id, vec![path], placement);
                }
            }
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
                        KeyboardShortcut::CycleTool => {
                            gui.remember_tool_size(self.input.tool());
                            if self.input.cycle_tool() {
                                self.pending_commands
                                    .push(AppCommand::SelectTool(self.input.tool()));
                                true
                            } else {
                                false
                            }
                        }
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
                let point_over_reference = self.screen == AppScreen::Editor
                    && gui.window_point_over_reference(self.input.cursor_position());
                let primary_press_over_reference = point_over_reference
                    && matches!(
                        &event,
                        WindowEvent::MouseInput {
                            state: ElementState::Pressed,
                            button: MouseButton::Left,
                            ..
                        }
                    );
                // Reference images consume pointer input in egui, but their scroll events should
                // still zoom the canvas beneath them.
                let wheel_over_reference =
                    point_over_reference && matches!(&event, WindowEvent::MouseWheel { .. });
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
                    && (self.input.captures_drag_event(&event)
                        || wheel_over_reference
                        || (!egui_consumed && !primary_press_over_reference))
                    && let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_mut())
                {
                    let brush_size_range = gui.brush_size_range();
                    let previous_tool = self.input.tool();
                    needs_redraw |= self.input.handle_event(
                        &event,
                        paint,
                        &mut gui.brush,
                        brush_size_range,
                        gui.stroke_smoothing,
                        &self.pressure_state,
                    );
                    if self.input.tool() != previous_tool {
                        gui.remember_tool_size(previous_tool);
                        self.pending_commands
                            .push(AppCommand::SelectTool(self.input.tool()));
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

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Command(command) => self.pending_commands.push(command),
            AppEvent::Pen(event) => {
                self.handle_pen_event(event_loop, event);
                return;
            }
            AppEvent::AutosaveWake
            | AppEvent::BrushImportWake
            | AppEvent::ExportWake
            | AppEvent::ReferenceImportWake
            | AppEvent::ReferenceLoadWake => {}
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
    fn handle_pen_event(&mut self, event_loop: &ActiveEventLoop, event: PenEvent) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        match event {
            PenEvent::Down { pressure, .. } => {
                self.pressure_state.note_pen_pressure(pressure, true, true);
            }
            PenEvent::Motion {
                pressure, contact, ..
            } => {
                self.pressure_state
                    .note_pen_pressure(pressure, contact, true);
            }
            PenEvent::Up => {}
            PenEvent::Leave => {}
        }
        for window_event in input::window_events_for_pen(event, window.scale_factor()) {
            self.window_event(event_loop, window.id(), window_event);
        }
        match event {
            PenEvent::Up => {
                self.pressure_state.end_pen_contact(true);
            }
            PenEvent::Leave => {
                self.pressure_state.clear_pen();
            }
            PenEvent::Down { .. } | PenEvent::Motion { .. } => {}
        }
    }

    fn new(
        settings: SettingsController,
        native_menu: NativeMenu,
        gallery: GalleryController,
        autosave: AutosaveController,
        export: ExportController,
        imports: ImportControllers,
        pen: PenControllers,
    ) -> Self {
        Self {
            window: None,
            paint: None,
            gui: None,
            input: PaintInputController::default(),
            pressure_state: PressureStateHandle::default(),
            _pressure_monitor: None,
            tablet_monitor: None,
            windows_pen_monitor: None,
            windows_pen_router: pen.windows_router,
            pen_proxy: pen.proxy,
            next_repaint: None,
            pending_commands: Vec::new(),
            settings,
            native_menu,
            gallery,
            autosave,
            export,
            brush_import: imports.brush,
            references: ReferenceBoard::default(),
            reference_import: imports.reference,
            reference_load: imports.reference_load,
            pending_reference_load: None,
            screen: AppScreen::Gallery,
            pending_gallery: false,
            pending_new_artwork: None,
            pending_exit: false,
        }
    }

    fn navigation_pending(&self) -> bool {
        self.pending_gallery || self.pending_exit
    }

    fn request_exit(&mut self) {
        if self.screen == AppScreen::Editor {
            if let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref()) {
                self.input.finish_document_interaction(paint, gui.brush);
            }
            let clean = self
                .paint
                .as_ref()
                .is_some_and(|paint| self.autosave.is_clean(paint, &self.references));
            if !clean {
                self.autosave.request_save();
            }
        }
        if let Some(gui) = self.gui.as_mut() {
            gui.close_new_artwork_dialog();
        }
        self.pending_exit = true;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn render(&mut self, window: &Window, event_loop: &ActiveEventLoop) {
        let mut app_action_processed = self.process_export_completion();
        app_action_processed |= self.process_brush_import_completion();
        app_action_processed |= self.process_reference_import_completions();
        app_action_processed |= self.process_reference_load_completions();
        app_action_processed |= self.process_pending_commands();
        let mut brush_switched = self.apply_pending_brush_change();

        if self.pending_exit && self.screen == AppScreen::Gallery && !self.export.is_exporting() {
            event_loop.exit();
            return;
        }
        if self.screen == AppScreen::Editor
            && let Some(paint) = self.paint.as_ref()
        {
            app_action_processed |= self.autosave.update(paint, &self.references);
            if self.pending_exit
                && !self.reference_load.is_loading()
                && self.autosave.is_clean(paint, &self.references)
                && !self.export.is_exporting()
            {
                event_loop.exit();
                return;
            }
            if self.pending_gallery
                && !self.reference_load.is_loading()
                && self.autosave.is_clean(paint, &self.references)
            {
                let new_size = self.pending_new_artwork;
                self.finish_gallery_navigation();
                if let Some(size) = new_size {
                    self.create_artwork(size);
                }
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
                    let status = self.autosave.status(paint, &self.references);
                    let pending_navigation = if self.reference_load.is_loading() {
                        None
                    } else if self.pending_exit {
                        Some("Closing Chromazen")
                    } else if self.pending_new_artwork.is_some() {
                        Some("Creating New Artwork")
                    } else if self.pending_gallery {
                        Some("Returning to Gallery")
                    } else {
                        None
                    };
                    gui.run_editor(
                        window,
                        EditorUiState {
                            layers: &layer_snapshot,
                            tool: self.input.tool(),
                            brush_resize_label,
                            eyedropper_indicator,
                            save_status: status,
                            pending_navigation,
                            brush_import_dialog_delay: self.brush_import.dialog_delay(),
                            reference_import_dialog_delay: self.reference_import.dialog_delay(),
                            reference_load_dialog_delay: self.reference_load.dialog_delay(),
                            references: self.references.images(),
                            workspace_view: paint.view_snapshot(),
                        },
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
            app_action_processed |= self.autosave.update(paint, &self.references);
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
                AppCommand::Undo => self.undo(),
                AppCommand::Redo => self.redo(),
                AppCommand::RotateCanvasLeft => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.rotate_canvas_view(-std::f32::consts::FRAC_PI_2);
                    }
                }
                AppCommand::RotateCanvasRight => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.rotate_canvas_view(std::f32::consts::FRAC_PI_2);
                    }
                }
                AppCommand::ResetCanvasRotation => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.reset_canvas_rotation();
                    }
                }
                AppCommand::ToggleCanvasFlipHorizontal => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.toggle_canvas_flip_horizontal();
                    }
                }
                AppCommand::ToggleCanvasFlipVertical => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.toggle_canvas_flip_vertical();
                    }
                }
                AppCommand::RequestCanvasResize => {
                    if self.screen == AppScreen::Editor
                        && let Some(size) = self.paint.as_ref().map(PaintRenderer::document_size)
                        && let Some(gui) = self.gui.as_mut()
                    {
                        gui.open_canvas_resize_dialog(size);
                    }
                }
                AppCommand::ResizeCanvas { width, height } => {
                    if let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref()) {
                        self.input.finish_document_interaction(paint, gui.brush);
                    }
                    let result = self
                        .paint
                        .as_mut()
                        .ok_or_else(|| "the paint renderer is unavailable".to_owned())
                        .and_then(|paint| paint.resize_canvas_centered([width, height]));
                    if let Err(error) = result
                        && let Some(gui) = self.gui.as_mut()
                    {
                        gui.show_error(error);
                    }
                }
                AppCommand::SelectTool(tool) => {
                    self.input.select_tool(tool);
                    if self.input.tool() != tool {
                        continue;
                    }
                    let id = self
                        .gui
                        .as_ref()
                        .map(|gui| gui.brush_for_tool(tool).to_owned());
                    if let Some(id) = id {
                        self.process_settings_commands(vec![SettingsCommand::SwitchBrush {
                            tool,
                            id,
                            reset_size: false,
                        }]);
                    }
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
                AppCommand::RenameLayer { id, name } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.rename_layer(id, &name);
                    }
                }
                AppCommand::MergeLayerDown(id) => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.merge_layer_down(id);
                    }
                }
                AppCommand::SetLayerClipped { id, clipped } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.set_layer_clipped(id, clipped);
                    }
                }
                AppCommand::SetLayerVisibility { id, visible } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.set_layer_visibility(id, visible);
                    }
                }
                AppCommand::SetLayerOpacity { id, opacity } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.set_layer_opacity(id, opacity);
                    }
                }
                AppCommand::CommitLayerOpacity { id, before, after } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.commit_layer_opacity(id, before, after);
                    }
                }
                AppCommand::MoveLayer {
                    dragged,
                    target,
                    edge,
                } => {
                    if let Some(paint) = self.paint.as_mut() {
                        paint.move_layer_relative(dragged, target, edge);
                    }
                }
                AppCommand::AddReferences => {
                    if self.screen != AppScreen::Editor || self.reference_load.is_loading() {
                        continue;
                    }
                    let paths = choose_reference_paths();
                    if let Some(artwork_id) = self.autosave.artwork_id().cloned() {
                        self.reference_import.start(artwork_id, paths, None);
                    }
                }
                AppCommand::SetReferenceTransform { id, position, size } => {
                    self.references.set_transform(id, position, size);
                }
                AppCommand::ToggleReferenceLocked(id) => {
                    self.references.toggle_locked(id);
                }
                AppCommand::DeleteReference(id) => {
                    self.references.remove(id);
                }
                AppCommand::SetBrushColor(color) => {
                    self.autosave.set_brush_color(color);
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
                AppCommand::SwitchBrush { tool, id } => {
                    self.process_settings_commands(vec![SettingsCommand::SwitchBrush {
                        tool,
                        id,
                        reset_size: true,
                    }]);
                }
                AppCommand::ImportBrushes => {
                    if self.screen != AppScreen::Editor {
                        continue;
                    }
                    let paths = choose_abr_paths();
                    self.brush_import.start(self.input.tool(), paths);
                }
                AppCommand::SaveSettings => {
                    let Some((brush, tool_brushes, tool_sizes)) =
                        self.gui.as_ref().map(GuiLayer::settings_snapshot)
                    else {
                        continue;
                    };
                    self.process_settings_commands(vec![SettingsCommand::Save {
                        brush,
                        tool_brushes,
                        tool_sizes,
                    }]);
                }
                AppCommand::ReloadConfiguration => {
                    self.process_settings_commands(vec![SettingsCommand::ReloadFromDisk(
                        self.input.tool(),
                    )]);
                }
                AppCommand::ResetBrush => {
                    if let Some(gui) = self.gui.as_mut() {
                        gui.reset_brush();
                    }
                }
                AppCommand::OpenConfigDirectory => {
                    self.process_settings_commands(vec![SettingsCommand::OpenConfigDirectory]);
                }
                AppCommand::NewArtwork => {
                    if !self.navigation_pending()
                        && let Some(gui) = self.gui.as_mut()
                    {
                        gui.open_new_artwork_dialog();
                    }
                }
                AppCommand::CreateArtwork { width, height } => {
                    if !self.navigation_pending() {
                        self.create_artwork([width, height]);
                    }
                }
                AppCommand::OpenArtwork(id) => self.open_artwork(&id),
                AppCommand::SaveArtwork => {
                    if self.screen == AppScreen::Editor {
                        self.autosave.request_save();
                    }
                }
                AppCommand::ExportPng => {
                    if self.screen != AppScreen::Editor || self.export.is_exporting() {
                        continue;
                    }
                    if let (Some(paint), Some(gui)) = (self.paint.as_mut(), self.gui.as_ref()) {
                        self.input.finish_document_interaction(paint, gui.brush);
                    }
                    let title = self.autosave.artwork_title().unwrap_or("Untitled");
                    let Some(path) = choose_export_path(title) else {
                        continue;
                    };
                    let result = self
                        .paint
                        .as_ref()
                        .ok_or_else(|| "the paint renderer is unavailable".to_owned())
                        .and_then(|paint| self.export.start(path, paint));
                    if let Err(error) = result
                        && let Some(gui) = self.gui.as_mut()
                    {
                        gui.show_error(error);
                    }
                }
                AppCommand::ShowGallery => {
                    if self.screen == AppScreen::Editor {
                        if let Some(gui) = self.gui.as_mut() {
                            gui.close_new_artwork_dialog();
                            gui.close_canvas_resize_dialog();
                        }
                        self.pending_gallery = true;
                        self.pending_new_artwork = None;
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
                    self.pending_new_artwork = None;
                    self.pending_exit = false;
                }
                AppCommand::Quit => self.request_exit(),
            }
        }
        self.sync_history_menu();
        true
    }

    fn process_brush_import_completion(&mut self) -> bool {
        let Some(completion) = self.brush_import.take_completion() else {
            return false;
        };
        let imported_count = completion.imported_ids.len();
        if let Some(first_id) = completion.imported_ids.first() {
            self.process_settings_commands(vec![SettingsCommand::SwitchBrush {
                tool: completion.tool,
                id: first_id.clone(),
                reset_size: true,
            }]);
        }

        let mut details = completion.warnings;
        details.extend(completion.errors);
        if let Some(gui) = self.gui.as_mut() {
            if imported_count == 0 {
                gui.show_error(if details.is_empty() {
                    "No brushes were imported".to_owned()
                } else {
                    details.join("\n")
                });
            } else if details.is_empty() {
                gui.show_success(format!(
                    "Imported {imported_count} Photoshop brush{}",
                    if imported_count == 1 { "" } else { "es" }
                ));
            } else {
                gui.show_error(format!(
                    "Imported {imported_count} Photoshop brush{} with warnings:\n{}",
                    if imported_count == 1 { "" } else { "es" },
                    details.join("\n")
                ));
            }
        }
        true
    }

    fn process_reference_import_completions(&mut self) -> bool {
        let mut changed = false;
        while let Some(completion) = self.reference_import.take_completion() {
            changed = true;
            if self.autosave.artwork_id() != Some(&completion.artwork_id) {
                continue;
            }
            let base = completion.placement.unwrap_or_else(|| {
                self.paint.as_ref().map_or([100.0, 100.0], |paint| {
                    [paint.document_size()[0] as f32 + 100.0, 100.0]
                })
            });
            for (index, image) in completion.images.into_iter().enumerate() {
                let offset = index as f32 * 32.0;
                self.references
                    .add(image, [base[0] + offset, base[1] + offset]);
            }
            if !completion.errors.is_empty()
                && let Some(gui) = self.gui.as_mut()
            {
                gui.show_error(completion.errors.join("\n"));
            }
        }
        if changed {
            self.sync_history_menu();
        }
        changed
    }

    fn process_reference_load_completions(&mut self) -> bool {
        let mut changed = false;
        while let Some(completion) = self.reference_load.take_completion() {
            changed = true;
            let Some(pending) = self
                .pending_reference_load
                .take_if(|pending| pending.id == completion.artwork_id)
            else {
                continue;
            };
            self.references.load(completion.references);
            self.autosave.begin_loaded(
                pending.id,
                pending.title,
                pending.paint_versions,
                self.references.versions(),
                pending.brush_color,
            );
            if !completion.warnings.is_empty()
                && let Some(gui) = self.gui.as_mut()
            {
                gui.show_error(completion.warnings.join("\n"));
            }
        }
        changed
    }

    fn process_export_completion(&mut self) -> bool {
        let Some(completion) = self.export.take_completion() else {
            return false;
        };
        if let Some(gui) = self.gui.as_mut() {
            match completion.result {
                Ok(()) => {
                    gui.show_success(format!("Exported PNG to {}", completion.path.display()))
                }
                Err(error) => {
                    self.pending_exit = false;
                    gui.show_error(error);
                }
            }
        }
        self.sync_history_menu();
        true
    }

    fn create_artwork(&mut self, size: [u32; 2]) {
        if self.screen == AppScreen::Editor {
            self.pending_gallery = true;
            self.pending_new_artwork = Some(size);
            self.autosave.request_save();
            return;
        }
        let Some(paint) = self.paint.as_mut() else {
            return;
        };
        if let Err(error) = paint.reset_document(size) {
            if let Some(gui) = self.gui.as_mut() {
                gui.show_error(error);
            }
            return;
        }
        let id = crate::artwork::ArtworkId::new();
        self.references.clear();
        self.pending_reference_load = None;
        let brush_color = self
            .gui
            .as_ref()
            .map(|gui| gui.brush.color.to_array())
            .unwrap_or([170, 187, 204, 255]);
        self.autosave
            .begin_new(id, "Untitled".to_owned(), brush_color);
        self.screen = AppScreen::Editor;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title("Untitled • Chromazen");
        }
    }

    fn open_artwork(&mut self, id: &crate::artwork::ArtworkId) {
        let Some(constraints) = self
            .paint
            .as_ref()
            .map(PaintRenderer::canvas_size_constraints)
        else {
            return;
        };
        let opened = match self.gallery.open(id, constraints) {
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
        if let Some(gui) = self.gui.as_mut() {
            gui.set_brush_color(opened.document.brush_color);
        }
        self.references.clear();
        self.autosave.clear();
        self.screen = AppScreen::Editor;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(&format!("{} • Chromazen", opened.title));
        }
        if opened.reference_sources.is_empty() {
            self.autosave.begin_loaded(
                opened.id,
                opened.title,
                versions,
                self.references.versions(),
                opened.document.brush_color,
            );
        } else {
            self.pending_reference_load = Some(PendingReferenceLoad {
                id: opened.id.clone(),
                title: opened.title,
                paint_versions: versions,
                brush_color: opened.document.brush_color,
            });
            self.reference_load
                .start(opened.id, opened.reference_sources);
        }
    }

    fn finish_gallery_navigation(&mut self) {
        self.gallery.refresh();
        self.autosave.clear();
        self.references.clear();
        self.pending_reference_load = None;
        self.screen = AppScreen::Gallery;
        self.pending_gallery = false;
        self.pending_new_artwork = None;
        self.pending_exit = false;
        if let Some(window) = self.window.as_ref() {
            window.set_title(WINDOW_TITLE);
        }
        self.sync_history_menu();
    }

    fn undo(&mut self) {
        if let Some(paint) = self.paint.as_mut() {
            paint.undo();
        }
    }

    fn redo(&mut self) {
        if let Some(paint) = self.paint.as_mut() {
            paint.redo();
        }
    }

    fn sync_history_menu(&self) {
        let (can_undo, can_redo) = (self.screen == AppScreen::Editor)
            .then_some(self.paint.as_ref())
            .flatten()
            .map_or((false, false), |paint| (paint.can_undo(), paint.can_redo()));
        self.native_menu.set_history_enabled(can_undo, can_redo);
        let in_editor = self.screen == AppScreen::Editor;
        self.native_menu.set_document_enabled(in_editor);
        self.native_menu
            .set_export_enabled(in_editor && !self.export.is_exporting());
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
        let is_rotating_canvas = self.input.is_rotating_canvas();
        let is_pan_modifier_active = self.input.is_pan_modifier_active();
        let is_eyedropper_active = self.input.is_eyedropper_active();
        let brush_pressure = self.pressure_state.brush_pressure();
        let paint = self.paint.as_mut()?;
        let gui = self.gui.as_mut()?;
        let pointer_over_ui = gui.context.is_pointer_over_egui();
        let pointer_over_reference =
            self.screen == AppScreen::Editor && gui.pointer_over_reference();
        let reference_drag_active = self.screen == AppScreen::Editor && gui.reference_drag_active();
        let reference_resize_active =
            self.screen == AppScreen::Editor && gui.reference_resize_active();
        let pointer_over_ui_or_reference =
            pointer_over_ui || pointer_over_reference || reference_drag_active;
        let brush_cursor = brush_resize_pos
            .filter(|_| resize_is_anchored || !pointer_over_ui_or_reference)
            .map(|center| BrushCursor {
                center,
                diameter: gui.brush.size,
            })
            .or_else(|| {
                cursor_pos
                    .filter(|_| !pointer_over_ui_or_reference)
                    .map(|center| BrushCursor {
                        center,
                        diameter: gui.brush.radius(brush_pressure) * 2.0,
                    })
            });
        let repaint_delay = ui::repaint_delay(&full_output);
        gui.state
            .handle_platform_output(window, full_output.platform_output);
        if reference_resize_active {
            window.set_cursor(CursorIcon::NwseResize);
        } else if reference_drag_active || is_panning || is_rotating_canvas {
            window.set_cursor(CursorIcon::Grabbing);
        } else if is_pan_modifier_active && !pointer_over_ui_or_reference {
            window.set_cursor(CursorIcon::Grab);
        }
        let eyedropper_over_canvas = is_eyedropper_active && !pointer_over_ui_or_reference;
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
        let tool = change.tool;
        let reset_size = change.reset_size;
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
                    tool,
                    self.settings.active_brush(),
                    completed.catalog,
                    completed.reloaded,
                    reset_size,
                );
                if completed.reloaded {
                    gui.settings_reloaded(self.settings.config(), tool);
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
    let windows_pen_router = WindowsPenRouter::new();
    let mut event_loop_builder = EventLoop::<AppEvent>::with_user_event();
    windows_pen_router.install_hook(&mut event_loop_builder);
    let event_loop = event_loop_builder
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
    let pen_proxy = event_loop.create_proxy();
    let autosave = AutosaveController::new(autosave_store, wake);
    let export_proxy = event_loop.create_proxy();
    let export = ExportController::new(Arc::new(move || {
        let _ = export_proxy.send_event(AppEvent::ExportWake);
    }));
    let brush_import_proxy = event_loop.create_proxy();
    let brush_import = BrushImportController::new(Arc::new(move || {
        let _ = brush_import_proxy.send_event(AppEvent::BrushImportWake);
    }));
    let reference_import_proxy = event_loop.create_proxy();
    let reference_import = ReferenceImportController::new(Arc::new(move || {
        let _ = reference_import_proxy.send_event(AppEvent::ReferenceImportWake);
    }));
    let reference_load_proxy = event_loop.create_proxy();
    let reference_load = ReferenceLoadController::new(Arc::new(move || {
        let _ = reference_load_proxy.send_event(AppEvent::ReferenceLoadWake);
    }));

    let mut app = App::new(
        SettingsController::load(),
        native_menu,
        gallery,
        autosave,
        export,
        ImportControllers {
            brush: brush_import,
            reference: reference_import,
            reference_load,
        },
        PenControllers {
            proxy: pen_proxy,
            windows_router: windows_pen_router,
        },
    );
    event_loop.run_app(&mut app).expect("event loop error");
}
