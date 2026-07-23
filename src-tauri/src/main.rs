mod desktop;
mod input_adapter;
mod raw_event_plugin;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{SyncSender, sync_channel},
};

use chromazen::{
    app::settings::SettingsController,
    platform::{MacosPressureMonitor, PressureStateHandle},
    protocol::UiCommand,
    renderer::PaintRenderer,
};
use tauri::{
    LogicalPosition, LogicalSize, State, WebviewUrl, webview::WebviewBuilder, window::WindowBuilder,
};

use self::{desktop::NativeMenu, raw_event_plugin::RawPaintPluginBuilder};

const WINDOW_WIDTH: f64 = 1_280.0;
const WINDOW_HEIGHT: f64 = 900.0;
const CONTROLS_WIDTH: f64 = 300.0;
const CONTROL_QUEUE_CAPACITY: usize = 256;

struct ControlSender(SyncSender<UiCommand>);

#[tauri::command]
fn dispatch(command: UiCommand, sender: State<'_, ControlSender>) -> Result<(), String> {
    sender
        .0
        .try_send(command)
        .map_err(|error| format!("control queue unavailable: {error}"))
}

fn main() {
    env_logger::init();

    let (command_sender, command_receiver) = sync_channel(CONTROL_QUEUE_CAPACITY);
    let settings = SettingsController::load();
    let mut app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ControlSender(command_sender))
        .invoke_handler(tauri::generate_handler![dispatch])
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");
    let native_menu = NativeMenu::new(&app).expect("failed to build native application menu");
    app.set_menu(native_menu.menu)
        .expect("failed to install native application menu");
    app.on_menu_event(desktop::handle_menu_event);
    let window = Arc::new(
        WindowBuilder::new(&app, "main")
            .title("Chromazen")
            .resizable(true)
            .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
            .build()
            .expect("failed to create native paint window"),
    );
    let size = window.inner_size().expect("failed to read window size");
    let scale_factor = window
        .scale_factor()
        .expect("failed to read window scale factor");
    let mut paint = pollster::block_on(PaintRenderer::new(
        window.clone(),
        [size.width, size.height],
        settings.active_brush(),
    ))
    .expect("failed to initialize native wgpu paint renderer");
    paint.set_canvas_viewport_size([
        size.width
            .saturating_sub((CONTROLS_WIDTH * scale_factor).round() as u32),
        size.height,
    ]);
    paint.fit_to_screen();

    let controls = window
        .add_child(
            WebviewBuilder::new("controls", WebviewUrl::App("index.html".into())),
            LogicalPosition::new(WINDOW_WIDTH - CONTROLS_WIDTH, 0.0),
            LogicalSize::new(CONTROLS_WIDTH, WINDOW_HEIGHT),
        )
        .expect("failed to create bounded controls webview");

    let pressure_state = PressureStateHandle::default();
    let pressure_redraw = Arc::new(AtomicBool::new(false));
    let pressure_redraw_callback = pressure_redraw.clone();
    let pressure_monitor =
        MacosPressureMonitor::install(window.clone(), pressure_state.clone(), move || {
            pressure_redraw_callback.store(true, Ordering::Release)
        })
        .expect("failed to initialize pressure monitor");
    if let Some(pressure_monitor) = pressure_monitor {
        // AppKit local monitor tokens are main-thread-only, while Tauri requires
        // runtime plugins to be Send. The monitor intentionally lives until exit.
        std::mem::forget(pressure_monitor);
    }

    app.wry_plugin(RawPaintPluginBuilder::new(
        paint,
        controls,
        CONTROLS_WIDTH,
        scale_factor,
        command_receiver,
        settings,
        pressure_state,
        pressure_redraw,
        native_menu.history,
    ));
    app.run(|_, _| {});
}
