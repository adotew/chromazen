mod app;
mod config;
mod gpu;
mod paint;
mod platform;
mod renderer;

fn main() {
    env_logger::init();
    app::run();
}
