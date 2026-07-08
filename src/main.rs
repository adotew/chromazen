mod app;
mod macos_pressure;
mod renderer;

fn main() {
    env_logger::init();
    app::run();
}
