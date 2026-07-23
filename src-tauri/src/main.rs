mod raw_event_plugin;

use std::sync::Arc;

use chromazen::{config::LoadedBrushPreset, renderer::PaintRenderer};
use tauri::{
    LogicalPosition, LogicalSize, WebviewUrl, webview::WebviewBuilder, window::WindowBuilder,
};

use raw_event_plugin::RawPaintPluginBuilder;

const WINDOW_WIDTH: f64 = 1_280.0;
const WINDOW_HEIGHT: f64 = 900.0;
const CONTROLS_WIDTH: f64 = 300.0;

fn main() {
    env_logger::init();

    let mut app = tauri::Builder::default()
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");
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
        &LoadedBrushPreset::bundled_charcoal(),
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

    app.wry_plugin(RawPaintPluginBuilder::new(
        paint,
        controls,
        CONTROLS_WIDTH,
        scale_factor,
    ));
    app.run(|_, _| {});
}
