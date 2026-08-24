mod autosave;
mod brush_import;
mod command;
mod commands;
mod completions;
mod export;
mod frame;
mod gallery;
mod input;
mod menu;
mod navigation;
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
    command::{
        AppCommand, EditorCommand, GalleryCommand, NavigationCommand,
        SettingsCommand as AppSettingsCommand, UiCommand,
    },
    export::{ExportController, choose_export_path},
    gallery::GalleryController,
    input::{EditorTool, KeyboardShortcut, PaintInputController},
    menu::NativeMenu,
    reference_import::{ReferenceImportController, choose_reference_paths},
    reference_load::ReferenceLoadController,
    references::ReferenceBoard,
    settings::{SettingsCommand, SettingsController, SettingsEffect},
    ui::{BrushResizeLabel, EditorUiState, EyedropperIndicator, GuiLayer},
};
use crate::{
    paint::PaintTool,
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
    GalleryWake,
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
            WindowEvent::RedrawRequested => self.redraw(window.as_ref(), event_loop),
            WindowEvent::DroppedFile(path)
                if self.screen == AppScreen::Editor
                    && !self.has_pending_navigation()
                    && !self.reference_load.is_loading() =>
            {
                let placement = self
                    .paint
                    .as_ref()
                    .map(|paint| paint.window_to_workspace(self.input.cursor_position()));
                if let Some(artwork_id) = self.autosave.artwork_id().cloned() {
                    self.reference_import
                        .start(artwork_id, vec![path], placement);
                }
            }
            event => {
                let navigation_pending = self.has_pending_navigation();
                let Some(gui) = self.gui.as_mut() else {
                    return;
                };
                let cursor_changed = self.input.update_pointer_and_modifier_state(&event);
                let canvas_crop_active =
                    self.screen == AppScreen::Editor && gui.canvas_crop_active();
                if self.screen == AppScreen::Editor
                    && !navigation_pending
                    && !canvas_crop_active
                    && let Some(shortcut) = self.input.keyboard_shortcut(&event)
                {
                    let changed = match shortcut {
                        KeyboardShortcut::TogglePanels => {
                            gui.toggle_panels();
                            true
                        }
                        KeyboardShortcut::CycleTool => {
                            if let Some(tool) = self.input.tool().paint_tool() {
                                gui.store_current_brush_settings_for_tool(tool);
                            }
                            if self.input.cycle_tool() {
                                self.pending_commands.push(AppCommand::Editor(
                                    EditorCommand::SelectTool(self.input.tool()),
                                ));
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

                let history_command = (self.screen == AppScreen::Editor
                    && !navigation_pending
                    && !canvas_crop_active
                    && !egui_consumed)
                    .then(|| self.input.command_for_platform_shortcut(&event))
                    .flatten();
                if let Some(command) = history_command {
                    self.pending_commands.push(command);
                    needs_redraw = true;
                } else if self.screen == AppScreen::Editor
                    && !navigation_pending
                    && !canvas_crop_active
                    && (self.input.has_active_document_drag()
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
                        &self.pressure_state,
                    );
                    if self.input.tool() != previous_tool {
                        if let Some(tool) = previous_tool.paint_tool() {
                            gui.store_current_brush_settings_for_tool(tool);
                        }
                        self.pending_commands
                            .push(AppCommand::Editor(EditorCommand::SelectTool(
                                self.input.tool(),
                            )));
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
            | AppEvent::ReferenceLoadWake
            | AppEvent::GalleryWake => {}
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
                self.pressure_state.reset_pen_state();
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
    let gallery_proxy = event_loop.create_proxy();
    let gallery = GalleryController::discover(Arc::new(move || {
        let _ = gallery_proxy.send_event(AppEvent::GalleryWake);
    }));
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
