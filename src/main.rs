mod app;
mod brush;
mod constants;
mod input;
mod macos_pressure;
mod renderer;
mod ui;

fn main() {
    env_logger::init();
    app::run();
}
